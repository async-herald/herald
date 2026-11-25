//! Analyse locale avec le moteur `herald-engine`, le même que le backend.
//!
//! Même chaîne que l'analyse du service herald.codes, sur la machine :
//! 1. copie locale à jour (`workspace`) ;
//! 2. fichiers retenus avec les mêmes filtres (`collect`) ;
//! 3. règles officielles actives (catalogue de herald.codes) ;
//! 4. `herald.config.json` du projet appliqué, contexte Git lu localement ;
//! 5. `herald_engine::analyze`, puis regroupement comme `grouped_response`
//!    (7 catégories, note globale sur les catégories applicables).
//!
//! Aucun fichier n'est envoyé : seules la liste des règles est téléchargée et,
//! comme sur le backend, les noms et versions des dépendances sont vérifiés
//! auprès d'OSV et des registres de paquets.

pub mod collect;
pub mod cve;
pub mod fix_command;
pub mod local_rules;
pub mod lockfile;
pub mod monorepo;
pub mod rule_check;
pub mod rules;
pub mod run_command;
pub mod workspace;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::SharedString;
use herald_engine::builtins::{devops, testing::is_test_file};
use herald_engine::category::Category as EngineCategory;
use herald_engine::config::Config;
use herald_engine::language::Language;
use herald_engine::score::overall_score;
use herald_engine::types::{AnalyzeRequest, AnalyzeResponse, FileInput, Issue as EngineIssue};

use crate::model::{
    AnalysisReport, Category, CategoryScore, Issue, PackageScore, Severity, SourceFile, Summary,
    rule_title,
};

/// Fichier où le moteur range les relevés sur l'historique Git.
const GIT_PSEUDO_FILE: &str = "(git)";

/// Étape en cours, lue par l'interface pendant l'analyse.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Preparing,
    /// Chargement des règles de herald.codes.
    Rules,
    /// Lecture des fichiers retenus : `read` sur `total`.
    Reading {
        read: usize,
        total: usize,
    },
    Analyzing {
        files: usize,
    },
    /// Relevé des dépendances (fichiers de verrouillage).
    Dependencies,
    /// Calcul de la note et du rapport.
    Scoring,
}

impl Phase {
    pub fn label(&self) -> String {
        match self {
            Self::Preparing => "Préparation du projet…".to_owned(),
            Self::Rules => "Chargement des règles…".to_owned(),
            Self::Reading { total: 0, .. } => "Lecture des fichiers…".to_owned(),
            Self::Reading { read, total } => format!("Lecture des fichiers… {read} sur {total}"),
            Self::Analyzing { files } => format!("Analyse de {files} fichiers…"),
            Self::Dependencies => "Relevé des dépendances…".to_owned(),
            Self::Scoring => "Calcul de la note…".to_owned(),
        }
    }
}

/// Avancement partagé avec l'interface : étape, puis fichiers retenus (pour
/// l'animation qui parcourt le projet), et ce que le moteur trouve au fil de
/// l'analyse (fil d'annonces du héraut).
#[derive(Debug, Default)]
pub struct Progress {
    pub phase: Phase,
    pub files: Vec<String>,
    /// Règles téléchargées ; `Some(0)` : catalogue intégré au moteur (hors ligne).
    pub rules: Option<usize>,
    /// Fichiers dont l'analyse est terminée.
    pub done: usize,
    /// Fichiers terminés avec au moins un relevé.
    pub flagged: usize,
    /// Fichiers avec des relevés, dans l'ordre où leur analyse s'est terminée
    /// (les plus récents seulement, voir `MAX_FINDINGS`).
    pub findings: Vec<Finding>,
    /// Problèmes trouvés jusqu'ici, par catégorie : (problèmes, critiques).
    pub by_category: HashMap<Category, (usize, usize)>,
    /// Problèmes trouvés jusqu'ici, par gravité : critiques, attention, info.
    pub by_severity: [usize; 3],
    /// Dernier fichier dont l'analyse est terminée.
    pub last_file: Option<String>,
}

/// Fichiers avec relevés gardés pour le fil d'annonces.
const MAX_FINDINGS: usize = 50;

/// Un fichier dont l'analyse a relevé des problèmes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    /// Moment depuis le début de l'analyse des fichiers.
    pub at: Duration,
    pub count: usize,
    pub critical: usize,
    /// Titre de la règle du relevé le plus grave.
    pub top: String,
    pub severity: Severity,
}

impl Progress {
    /// Enregistre la fin de l'analyse d'un fichier et ses relevés.
    fn record(&mut self, path: &str, issues: &[EngineIssue], at: Duration) {
        self.done += 1;
        self.last_file = Some(path.to_owned());
        let Some(worst) = issues
            .iter()
            .min_by_key(|issue| severity_of(&issue.severity))
        else {
            return;
        };
        self.flagged += 1;
        let mut critical = 0;
        for issue in issues {
            self.by_severity[severity_of(&issue.severity) as usize] += 1;
            let entry = self
                .by_category
                .entry(category_of(&issue.category))
                .or_default();
            entry.0 += 1;
            if severity_of(&issue.severity) == Severity::Critical {
                entry.1 += 1;
                critical += 1;
            }
        }
        self.findings.push(Finding {
            path: path.to_owned(),
            at,
            count: issues.len(),
            critical,
            top: rule_title(&worst.rule),
            severity: severity_of(&worst.severity),
        });
        if self.findings.len() > MAX_FINDINGS {
            self.findings.remove(0);
        }
    }
}

pub type SharedPhase = Arc<Mutex<Progress>>;

/// Ce qu'il faut pour analyser un projet.
pub struct Request<'a> {
    pub path: &'a Path,
    pub remote: Option<&'a str>,
    /// Charger les règles de herald.codes (sinon : catalogue intégré au moteur).
    pub fetch_rules: bool,
    pub previous_score: Option<u8>,
}

/// Résultat d'une analyse : le rapport et les dépendances déclarées (pour les CVE).
pub struct Analysis {
    pub report: AnalysisReport,
    pub dependencies: Vec<cve::Dependency>,
}

/// Lance toute l'analyse. Bloquant : à appeler hors du fil d'interface.
pub fn analyze_project(request: Request<'_>, phase: &SharedPhase) -> Result<Analysis, String> {
    set_phase(phase, Phase::Preparing);
    workspace::prepare(request.path, request.remote)?;
    set_phase(phase, Phase::Rules);
    let (mut rule_sources, online) = if request.fetch_rules {
        rules::sources()
    } else {
        (local_rules::builtin_sources(), false)
    };
    // Règles écrites dans l'éditeur de l'app.
    merge_custom(&mut rule_sources, rules::custom_sources());
    if let Ok(mut progress) = phase.lock() {
        // `Some(0)` : catalogue intégré au moteur (hors ligne).
        progress.rules = Some(if online { rule_sources.len() } else { 0 });
    }

    set_phase(phase, Phase::Reading { read: 0, total: 0 });
    let started = Instant::now();
    let collected = collect::collect_files(request.path, max_files(), |read, total| {
        set_phase(phase, Phase::Reading { read, total });
    });
    let candidate_count = collected.candidates;
    let mut files = collected.files;
    let packages = detect_packages(request.path, &files);
    let mut config = Config {
        rule_sources,
        // En local, tout le projet est analysé, même au-delà du budget de temps du serveur.
        no_time_budget: true,
        ..Config::default()
    };
    collect::apply_repo_config(request.path, &mut config, &mut files);
    // Règles désactivées sur l'écran « Règles » (après herald.config.json, qui remplace `rules`).
    config.rules.disable.extend(rules::disabled_by_user());
    if files.is_empty() {
        return Err("Aucun fichier de code reconnu dans ce dossier.".to_owned());
    }

    if let Ok(mut progress) = phase.lock() {
        progress.files = files.iter().map(|file| file.path.clone()).collect();
        progress.phase = Phase::Analyzing { files: files.len() };
    }
    let mut engine_request = AnalyzeRequest {
        files,
        git: workspace::git_context(request.path),
    };
    // Chaque fichier terminé alimente le fil d'annonces de l'interface.
    let files_started = Instant::now();
    let on_file = |path: &str, issues: &[EngineIssue]| {
        if let Ok(mut progress) = phase.lock() {
            progress.record(path, issues, files_started.elapsed());
        }
    };
    let response = runtime().block_on(herald_engine::analyze_with_progress(
        &engine_request,
        &config,
        &on_file,
    ));
    let duration = started.elapsed();
    release_unneeded_code(&mut engine_request.files, &response);

    set_phase(phase, Phase::Dependencies);
    let dependencies = cve::dependencies(request.path, &engine_request.files);
    set_phase(phase, Phase::Scoring);
    let report = build_report(
        &engine_request.files,
        &response,
        request.previous_score,
        duration,
        candidate_count,
        &packages,
    );
    Ok(Analysis {
        report,
        dependencies,
    })
}

/// Analyse terminée : le code n'est plus utile que pour les fichiers avec des
/// relevés (extraits) et les manifestes (dépendances). Le reste est libéré
/// avant le calcul du rapport, pour garder l'analyse légère en mémoire.
fn release_unneeded_code(files: &mut [FileInput], response: &AnalyzeResponse) {
    let concerned: HashSet<&str> = response
        .files
        .iter()
        .filter(|file| !file.issues.is_empty())
        .map(|file| file.path.as_str())
        .collect();
    for file in files.iter_mut() {
        if !concerned.contains(file.path.as_str())
            && !herald_engine::dependencies::manifest::is_manifest(&file.path)
        {
            file.code = String::new();
        }
    }
}

/// Limite de fichiers choisie dans Réglages > Analyse (une valeur inconnue
/// revient à la limite par défaut).
pub fn max_files() -> usize {
    crate::store::text_setting(collect::MAX_FILES_SETTING)
        .and_then(|value| value.parse().ok())
        .filter(|limit| collect::FILE_LIMITS.contains(limit))
        .unwrap_or(collect::DEFAULT_MAX_FILES)
}

/// Ajoute les règles personnelles ; votre version d'une règle officielle la
/// remplace (même identifiant) au lieu de s'y ajouter.
fn merge_custom(sources: &mut Vec<String>, custom: Vec<String>) {
    let custom_ids: HashSet<String> = custom
        .iter()
        .filter_map(|source| herald_engine::dsl::parse_meta(source).ok().map(|m| m.id))
        .collect();
    sources.retain(|source| {
        herald_engine::dsl::parse_meta(source).map_or(true, |m| !custom_ids.contains(&m.id))
    });
    sources.extend(custom);
}

fn set_phase(phase: &SharedPhase, value: Phase) {
    if let Ok(mut progress) = phase.lock() {
        progress.phase = value;
    }
}

/// Le moteur est asynchrone et utilise `block_in_place` : il lui faut un runtime
/// Tokio multi-thread, créé une fois pour toutes.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("herald-engine")
            .build()
            .expect("runtime Tokio du moteur")
    })
}

fn category_of(category: &EngineCategory) -> Category {
    match category {
        EngineCategory::Security => Category::Security,
        EngineCategory::Quality => Category::Quality,
        EngineCategory::Git => Category::Git,
        EngineCategory::Devops => Category::Devops,
        EngineCategory::Pattern => Category::Pattern,
        EngineCategory::Dependencies => Category::Dependencies,
        EngineCategory::Testing => Category::Tests,
    }
}

fn severity_of(severity: &herald_engine::types::Severity) -> Severity {
    match severity {
        herald_engine::types::Severity::Error => Severity::Critical,
        herald_engine::types::Severity::Warning => Severity::Warning,
        herald_engine::types::Severity::Info => Severity::Info,
    }
}

/// `(erreurs, avertissements, infos)` par règle : l'entrée du barème du moteur.
fn breakdown<'a>(issues: impl IntoIterator<Item = &'a EngineIssue>) -> Vec<(u32, u32, u32)> {
    let mut per_rule: HashMap<&str, (u32, u32, u32)> = HashMap::new();
    for issue in issues {
        let slot = per_rule.entry(issue.rule.as_str()).or_default();
        match issue.severity {
            herald_engine::types::Severity::Error => slot.0 += 1,
            herald_engine::types::Severity::Warning => slot.1 += 1,
            herald_engine::types::Severity::Info => slot.2 += 1,
        }
    }
    per_rule.into_values().collect()
}

/// Même regroupement que `grouped_response` du backend.
fn build_report(
    files: &[FileInput],
    response: &AnalyzeResponse,
    previous_score: Option<u8>,
    duration: Duration,
    candidate_count: usize,
    packages: &[monorepo::Package],
) -> AnalysisReport {
    let total_files = files.len();
    let code: HashMap<&str, &str> = files
        .iter()
        .map(|file| (file.path.as_str(), file.code.as_str()))
        .collect();
    let flat: Vec<(&str, &EngineIssue)> = response
        .files
        .iter()
        .flat_map(|file| {
            file.issues
                .iter()
                .map(move |issue| (file.path.as_str(), issue))
        })
        // Sécurité : documentation, exemples, tests et bundles minifiés ne
        // sont pas la surface d'attaque du projet (voir `local_rules`).
        .filter(|(path, issue)| {
            issue.category != EngineCategory::Security
                || !local_rules::dismissed(
                    &issue.rule,
                    path,
                    code.get(path).copied().unwrap_or_default(),
                )
        })
        .collect();

    // DevOps / Tests ne comptent dans la note globale que s'il y a de quoi examiner.
    let has_devops_files = files
        .iter()
        .any(|f| devops::is_dockerfile(&f.path) || devops::is_ci_workflow(&f.path));
    let has_test_files = files.iter().any(|f| is_test_file(&f.path));
    let is_applicable = |category: Category| match category {
        Category::Devops => has_devops_files,
        Category::Tests => has_test_files,
        _ => true,
    };

    let mut categories = Vec::with_capacity(Category::ALL.len());
    let mut overall_issues: Vec<&EngineIssue> = Vec::new();
    for category in Category::ALL {
        let issues: Vec<&EngineIssue> = flat
            .iter()
            .map(|(_, issue)| *issue)
            .filter(|issue| category_of(&issue.category) == category)
            .collect();
        let applicable = is_applicable(category);
        let issue_count = issues.len() as u32;
        // Relevés « À revoir » : affichés, mais hors note.
        let issues: Vec<&EngineIssue> = issues
            .into_iter()
            .filter(|issue| !local_rules::needs_review(&issue.rule))
            .collect();
        categories.push(CategoryScore {
            category,
            score: overall_score(breakdown(issues.iter().copied()), total_files) as u8,
            issue_count,
            applicable,
        });
        if applicable {
            overall_issues.extend(issues);
        }
    }
    let score = overall_score(breakdown(overall_issues), total_files) as u8;

    let issues: Vec<Issue> = flat
        .iter()
        .map(|(path, issue)| Issue {
            rule: issue.rule.clone().into(),
            title: rule_title(&issue.rule).into(),
            severity: severity_of(&issue.severity),
            category: category_of(&issue.category),
            file: if *path == GIT_PSEUDO_FILE {
                "Historique Git".into()
            } else {
                SharedString::from(path.to_string())
            },
            line: (issue.line > 0).then_some(issue.line as u32),
            explanation: Some(issue.message.clone().into()),
            fix: None,
            references: None,
            review: local_rules::needs_review(&issue.rule),
        })
        .collect();

    // Code des fichiers concernés, pour l'extrait autour de la ligne (en mémoire seulement).
    let concerned: HashSet<&str> = flat.iter().map(|(path, _)| *path).collect();
    let sources = files
        .iter()
        .filter(|file| concerned.contains(file.path.as_str()))
        .map(|file| source_file(&file.path, &file.code))
        .collect();

    let language_count = files
        .iter()
        .filter_map(|file| Language::from_path(&file.path))
        .collect::<HashSet<_>>()
        .len();

    let package_scores = package_scores(packages, files, &flat, &is_applicable);

    let mut report = AnalysisReport::new(
        Summary {
            score,
            previous_score,
            file_count: total_files as u32,
            candidate_count: candidate_count.max(total_files) as u32,
            language_count: language_count as u32,
            duration_ms: duration.as_millis().min(u128::from(u32::MAX)) as u32,
            issue_count: response.total_issues as u32,
            analyzed_at: "à l'instant".into(),
            analyzed_at_unix: cve::unix_now(),
        },
        categories,
        issues,
        sources,
    );
    report.packages = package_scores;
    report
}

/// Paquets du monorepo (vide sinon), avant que le code ne soit libéré : les
/// noms viennent des manifestes, les déclarations de workspace de la racine.
fn detect_packages(root: &Path, files: &[FileInput]) -> Vec<monorepo::Package> {
    let extra: Vec<FileInput> = monorepo::WORKSPACE_FILES
        .iter()
        .filter(|name| !files.iter().any(|f| f.path == **name))
        .filter_map(|name| {
            let code = std::fs::read_to_string(root.join(name)).ok()?;
            Some(FileInput {
                path: (*name).to_owned(),
                code,
            })
        })
        .collect();
    monorepo::detect(files, &extra)
}

/// Note de chaque paquet (et des fichiers hors paquets), au même barème que la
/// note globale : catégories applicables, relevés « À revoir » exclus.
fn package_scores(
    packages: &[monorepo::Package],
    files: &[FileInput],
    flat: &[(&str, &EngineIssue)],
    is_applicable: &impl Fn(Category) -> bool,
) -> Vec<PackageScore> {
    if packages.is_empty() {
        return Vec::new();
    }
    let key = |path: &str| monorepo::package_of(packages, path).map(|p| p.path.as_str());
    let mut rows: Vec<(Option<&str>, String)> = packages
        .iter()
        .map(|p| (Some(p.path.as_str()), p.name.clone()))
        .collect();
    rows.push((None, "Hors paquets".to_owned()));
    rows.into_iter()
        .filter_map(|(path, name)| {
            let file_count = files.iter().filter(|f| key(&f.path) == path).count();
            if file_count == 0 {
                return None;
            }
            let issues: Vec<&EngineIssue> = flat
                .iter()
                .filter(|(file, _)| *file != GIT_PSEUDO_FILE && key(file) == path)
                .map(|(_, issue)| *issue)
                .collect();
            let critical_count = issues
                .iter()
                .filter(|i| matches!(i.severity, herald_engine::types::Severity::Error))
                .count();
            let scored = issues
                .iter()
                .copied()
                .filter(|i| is_applicable(category_of(&i.category)))
                .filter(|i| !local_rules::needs_review(&i.rule));
            Some(PackageScore {
                name: name.into(),
                path: path.unwrap_or_default().to_owned().into(),
                file_count: file_count as u32,
                score: overall_score(breakdown(scored), file_count) as u8,
                issue_count: issues.len() as u32,
                critical_count: critical_count as u32,
            })
        })
        .collect()
}

/// Extrait affichable d'un fichier.
fn source_file(path: &str, code: &str) -> SourceFile {
    SourceFile {
        path: path.to_owned().into(),
        lines: code
            .lines()
            .map(|line| SharedString::from(line.to_owned()))
            .collect(),
    }
}

/// Relit depuis le disque les fichiers cités par un rapport sauvegardé (le code
/// n'est pas stocké). Un fichier disparu ou trop gros n'a simplement pas d'extrait.
pub fn read_sources(root: &Path, issues: &[Issue]) -> Vec<SourceFile> {
    let files: HashSet<&str> = issues.iter().map(|issue| issue.file.as_ref()).collect();
    files
        .into_iter()
        .filter(|file| !file.starts_with('/') && !file.contains(".."))
        .filter_map(|file| {
            let full = root.join(file);
            let size = std::fs::metadata(&full).ok()?.len();
            if size > collect::MAX_FILE_BYTES {
                return None;
            }
            let code = std::fs::read_to_string(&full).ok()?;
            Some(source_file(file, &code))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, message: &str) -> String {
        format!(
            "rule {id} {{\n  meta {{\n    type: security\n    severity: warning\n    message: \"{message}\"\n  }}\n  match js: eval($X)\n}}\n"
        )
    }

    #[test]
    fn your_version_of_an_official_rule_replaces_it() {
        let mut sources = vec![rule("no-eval", "officielle"), rule("other", "officielle")];
        merge_custom(
            &mut sources,
            vec![rule("no-eval", "la mienne"), rule("mine", "la mienne")],
        );
        let ids: Vec<String> = sources
            .iter()
            .map(|s| herald_engine::dsl::parse_meta(s).unwrap().id)
            .collect();
        assert_eq!(
            ids,
            vec!["other", "no-eval", "mine"],
            "une seule no-eval, la vôtre"
        );
        let no_eval = sources.iter().find(|s| s.contains("no-eval")).unwrap();
        assert!(no_eval.contains("la mienne"));
    }

    #[test]
    fn analyzes_a_small_project_end_to_end() {
        let root = std::env::temp_dir().join(format!("herald-engine-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        // Fausse clé Stripe, écrite en morceaux pour que les analyses de secrets
        // (GitHub, gitleaks) ne la prennent pas pour une vraie.
        std::fs::write(
            root.join("src/config.js"),
            concat!(
                "const apiKey = \"sk_",
                "live_",
                "51HxQwErTyUiOpAsDfGhJkLzXcVbNm\";\neval(userInput);\n"
            ),
        )
        .unwrap();
        std::fs::write(root.join("README.md"), "# test\n").unwrap();

        let phase = SharedPhase::default();
        let analysis = analyze_project(
            Request {
                path: &root,
                remote: None,
                fetch_rules: false,
                previous_score: None,
            },
            &phase,
        )
        .expect("analyse");
        let report = analysis.report;
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(report.summary.file_count, 1, "README.md n'est pas analysé");
        assert!(
            !report.issues.is_empty(),
            "un secret et un eval sont relevés"
        );
        assert!(report.summary.score < 100);
        assert_eq!(report.projected_score(), report.summary.score);
        let progress = phase.lock().unwrap();
        // Dernière étape : la note, après les dépendances.
        assert_eq!(progress.phase, Phase::Scoring);
        let in_files = report
            .issues
            .iter()
            .filter(|issue| issue.file.as_ref() != GIT_PSEUDO_FILE)
            .count();
        assert_eq!(progress.by_severity.iter().sum::<usize>(), in_files);
        assert_eq!(progress.files, vec!["src/config.js".to_owned()]);
    }
}
