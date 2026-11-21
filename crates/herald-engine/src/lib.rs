pub mod builtins;
pub mod category;
pub mod config;
pub mod dependencies;
pub mod dsl;
pub mod helpers;
pub mod language;
pub mod parser;
pub mod prod_scope;
pub mod rules;
pub mod score;
pub mod semantic;
pub mod types;

use std::time::{Duration, Instant};

use rayon::prelude::*;
use tree_sitter::Tree;

use crate::config::Config;
use crate::language::Language;
use crate::types::{AnalyzeRequest, AnalyzeResponse, FileResult, Issue};

/// Budget pour l'étape la plus coûteuse (parsing + règles sur tous les
/// fichiers) : sans plafond, un monorepo à plusieurs milliers de fichiers
/// peut bloquer la requête bien au-delà de ce qui reste raisonnable. Coopératif
/// plutôt qu'un `tokio::time::timeout` autour de tout `analyze()` : ce dernier
/// arrêterait juste d'ATTENDRE sans stopper le calcul CPU déjà lancé dans
/// `block_in_place` (rayon continuerait à tourner en arrière-plan pour rien).
/// Ici, chaque fichier vérifie le budget avant de démarrer son propre
/// traitement : le travail déjà en cours va au bout, mais aucun nouveau
/// fichier n'est entamé une fois le budget dépassé.
const ANALYZE_BUDGET: Duration = Duration::from_secs(50);

pub async fn analyze(req: &AnalyzeRequest, config: &Config) -> AnalyzeResponse {
    analyze_with_progress(req, config, &|_, _| {}).await
}

/// Comme [`analyze`], en appelant `on_file` après l'analyse de chaque fichier,
/// avec ses relevés propres (hors analyses transverses, faites à la fin).
/// Les fichiers sont analysés en parallèle : `on_file` est appelé depuis les
/// fils de calcul, dans l'ordre où ils se terminent.
pub async fn analyze_with_progress(
    req: &AnalyzeRequest,
    config: &Config,
    on_file: &(dyn Fn(&str, &[Issue]) + Sync),
) -> AnalyzeResponse {
    // Règles de la base si fournies (data-driven), sinon catalogue compilé.
    let compiled = if config.rule_sources.is_empty() {
        None
    } else {
        Some(dsl::compile(&config.rule_sources)).filter(|c| !c.is_empty())
    };

    let deadline = Instant::now() + ANALYZE_BUDGET;

    let (mut per_file, cross) = tokio::task::block_in_place(|| {
        let computed: Vec<((Option<Language>, Option<Tree>), Vec<Issue>)> = req
            .files
            .par_iter()
            .map(|f| {
                // Budget dépassé : on n'entame plus de nouveau fichier, mais on
                // ne coupe pas ceux déjà en cours sur les autres threads.
                if Instant::now() >= deadline {
                    return ((None, None), Vec::new());
                }
                let lang = Language::from_path(&f.path);
                let tree = lang.and_then(|l| parser::parse(&f.code, l));
                let mut issues = match &compiled {
                    Some(set) => dsl::run_all_on(set, &f.code, tree.as_ref(), lang, |id| {
                        config.is_rule_enabled(id)
                    }),
                    None => rules::run_all(&f.code, tree.as_ref(), lang, config),
                };
                // path-based rules (e.g. .env / key files committed)
                issues.extend(rules::run_context(Some(&f.path), None, None, config));
                // Les règles « prod-only » (fiabilité) ne comptent pas sur du
                // code de test/bench/exemple, ni dans les blocs `#[cfg(test)]`.
                prod_scope::retain_prod(&mut issues, &f.path, tree.as_ref(), &f.code, lang);
                // DevOps (Dockerfile / pipelines CI) : ni l'un ni l'autre n'a de
                // langage reconnu, donc routées par nom de fichier plutôt que
                // par la DSL — hors périmètre de `retain_prod` (pas du code).
                issues.extend(rules::run_devops(&f.path, &f.code, config));
                on_file(&f.path, &issues);
                ((lang, tree), issues)
            })
            .collect();

        let (parsed, per_file): (Vec<(Option<Language>, Option<Tree>)>, Vec<Vec<Issue>>) =
            computed.into_iter().unzip();

        let cross = semantic::analyze(&req.files, &parsed, config);
        (per_file, cross)
    });

    if Instant::now() >= deadline {
        eprintln!(
            "[analyze] budget de {}s dépassé sur {} fichiers — rapport partiel",
            ANALYZE_BUDGET.as_secs(),
            req.files.len()
        );
    }

    let all_manifests: Vec<(usize, Vec<dependencies::Dependency>)> = req
        .files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| dependencies::manifest::parse(&f.path, &f.code).map(|deps| (i, deps)))
        .collect();

    // Cross-manifest (monorepo) : une même dépendance à versions divergentes
    // entre plusieurs manifests — a besoin de voir TOUS les manifests avant de
    // conclure, donc calculé à part du traitement par-fichier ci-dessous.
    let flat_deps: Vec<(usize, dependencies::Dependency)> = all_manifests
        .iter()
        .flat_map(|(i, deps)| deps.iter().cloned().map(move |d| (*i, d)))
        .collect();
    for (i, issue) in dependencies::to_duplicate_version_issues(&flat_deps) {
        per_file[i].push(issue);
    }

    let dep_futures = all_manifests.into_iter().map(|(i, deps)| {
        // Purement local (pas de réseau) : posé tout de suite, pas besoin
        // du dédoublonnage global ci-dessous (qui suppose le format de
        // message des relevés réseau).
        per_file[i].extend(dependencies::to_wildcard_issues(&deps));
        async move { (i, dependencies::to_issues(&dependencies::check_all(deps).await)) }
    });
    // Dédoublonnage GLOBAL par nom de dépendance : dans un workspace, un même
    // paquet peut figurer dans plusieurs manifests. On ne garde qu'un relevé par
    // dépendance (la pire sévérité), en conservant un ordre stable.
    let sev_rank = |s: &crate::types::Severity| match s {
        crate::types::Severity::Error => 2,
        crate::types::Severity::Warning => 1,
        crate::types::Severity::Info => 0,
    };
    let mut dep_order: Vec<String> = Vec::new();
    let mut dep_best: std::collections::HashMap<String, (usize, Issue)> = std::collections::HashMap::new();
    for (i, issues) in futures::future::join_all(dep_futures).await {
        for iss in issues {
            let name = iss.message.split(" (").next().unwrap_or("").to_string();
            match dep_best.get(&name) {
                Some((_, ex)) if sev_rank(&ex.severity) >= sev_rank(&iss.severity) => {}
                Some(_) => {
                    dep_best.insert(name, (i, iss));
                }
                None => {
                    dep_order.push(name.clone());
                    dep_best.insert(name, (i, iss));
                }
            }
        }
    }
    for name in dep_order {
        if let Some((i, iss)) = dep_best.remove(&name) {
            per_file[i].push(iss);
        }
    }

    let mut files: Vec<FileResult> = req
        .files
        .iter()
        .zip(per_file)
        .map(|(f, mut issues)| {
            if let Some(extra) = cross.get(&f.path) {
                issues.extend(extra.iter().cloned());
            }
            FileResult { path: f.path.clone(), issues }
        })
        .collect();

    // Repo-global git-metadata rules (branch / commit messages).
    if let Some(git) = &req.git {
        let mut meta = rules::run_context(None, git.branch.as_deref(), None, config);
        // Un message par commit, mais une seule remontée par règle : sinon un
        // historique répétitif (ex. 20 commits "wip") écraserait le score bien
        // plus qu'un dépôt avec un seul commit fautif.
        let mut flagged_rules = std::collections::HashSet::new();
        for commit in &git.commits {
            for issue in rules::run_context(None, None, Some(commit), config) {
                if flagged_rules.insert(issue.rule.clone()) {
                    meta.push(issue);
                }
            }
        }
        if !meta.is_empty() {
            files.push(FileResult { path: "(git)".to_string(), issues: meta });
        }
    }

    let total_issues: usize = files.iter().map(|f| f.issues.len()).sum();
    let score = score::calculate(files.iter().flat_map(|f| f.issues.iter()), req.files.len());

    AnalyzeResponse {
        files,
        total_issues,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::types::{FileInput, GitContext};

    fn git_issues(response: &AnalyzeResponse) -> Vec<&Issue> {
        response
            .files
            .iter()
            .find(|f| f.path == "(git)")
            .map(|f| f.issues.iter().collect())
            .unwrap_or_default()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn repeated_wip_commits_flag_the_rule_only_once() {
        let req = AnalyzeRequest {
            files: vec![FileInput { path: "a.js".into(), code: "const x = 1;".into() }],
            git: Some(GitContext {
                branch: None,
                commits: vec!["wip".into(), "wip: more".into(), "fixup! oops".into()],
            }),
        };
        let res = analyze(&req, &Config::default()).await;
        let wip_hits = git_issues(&res).iter().filter(|i| i.rule == "no-wip-commit").count();
        assert_eq!(wip_hits, 1, "20 wip-ish commits should not multiply the same rule's penalty");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn branch_and_commit_rules_both_run_from_git_context() {
        let req = AnalyzeRequest {
            files: vec![FileInput { path: "a.js".into(), code: "const x = 1;".into() }],
            git: Some(GitContext {
                branch: Some("johns-random-branch".into()),
                commits: vec!["feat: add login".into()],
            }),
        };
        let res = analyze(&req, &Config::default()).await;
        let rules: Vec<&str> = git_issues(&res).iter().map(|i| i.rule.as_str()).collect();
        assert!(rules.contains(&"branch-naming"), "non-conventional branch should be flagged");
        assert!(
            !rules.contains(&"conventional-commit"),
            "conventional commit message should not be flagged"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn no_git_context_means_no_git_metadata_issues() {
        let req = AnalyzeRequest {
            files: vec![FileInput { path: "a.js".into(), code: "const x = 1;".into() }],
            git: None,
        };
        let res = analyze(&req, &Config::default()).await;
        assert!(res.files.iter().all(|f| f.path != "(git)"));
    }
}