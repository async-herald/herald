pub mod builtins;
pub mod category;
pub mod config;
pub mod dependencies;
pub mod dsl;
pub mod helpers;
pub mod language;
pub mod parser;
pub mod rules;
pub mod score;
pub mod semantic;
pub mod types;

use rayon::prelude::*;
use tree_sitter::Tree;

use crate::config::Config;
use crate::language::Language;
use crate::types::{AnalyzeRequest, AnalyzeResponse, FileResult, Issue};

pub async fn analyze(req: &AnalyzeRequest, config: &Config) -> AnalyzeResponse {
    // Règles de la base si fournies (data-driven), sinon catalogue compilé.
    let compiled = if config.rule_sources.is_empty() {
        None
    } else {
        Some(dsl::compile(&config.rule_sources)).filter(|c| !c.is_empty())
    };

    let (mut per_file, cross) = tokio::task::block_in_place(|| {
        let computed: Vec<((Option<Language>, Option<Tree>), Vec<Issue>)> = req
            .files
            .par_iter()
            .map(|f| {
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
                ((lang, tree), issues)
            })
            .collect();

        let (parsed, per_file): (Vec<(Option<Language>, Option<Tree>)>, Vec<Vec<Issue>>) =
            computed.into_iter().unzip();

        let cross = semantic::analyze(&req.files, &parsed, config);
        (per_file, cross)
    });

    let dep_futures = req.files.iter().enumerate().filter_map(|(i, f)| {
        dependencies::manifest::parse(&f.path, &f.code)
            .map(|deps| async move { (i, dependencies::to_issues(&dependencies::check_all(deps).await)) })
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

    // Repo-global git-metadata rules (branch / commit message).
    if let Some(git) = &req.git {
        let meta = rules::run_context(None, git.branch.as_deref(), git.commit.as_deref(), config);
        if !meta.is_empty() {
            files.push(FileResult { path: "(git)".to_string(), issues: meta });
        }
    }

    let total_issues: usize = files.iter().map(|f| f.issues.len()).sum();
    let score = score::calculate(files.iter().flat_map(|f| f.issues.iter()));

    AnalyzeResponse {
        files,
        total_issues,
        score,
    }
}