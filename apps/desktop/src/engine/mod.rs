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
pub mod local_rules;
pub mod lockfile;
pub mod rules;
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
    AnalysisReport, Category, CategoryScore, Issue, Severity, SourceFile, Summary, rule_title,
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
    Reading,
    Analyzing {
        files: usize,
    },
}

impl Phase {
    pub fn label(&self) -> String {
        match self {
            Self::Preparing => "Préparation du projet…".to_owned(),
            Self::Rules => "Chargement des règles…".to_owned(),
            Self::Reading => "Lecture des fichiers…".to_owned(),
            Self::Analyzing { files } => format!("Analyse de {files} fichiers…"),
        }
    }
}

/// Avancement partagé avec l'interface : étape, puis fichiers retenus (pour
/// l'animation qui parcourt le projet).
#[derive(Debug, Default)]
pub struct Progress {
    pub phase: Phase,
    pub files: Vec<String>,
    /// Règles téléchargées ; `Some(0)` : catalogue intégré au moteur (hors ligne).
    pub rules: Option<usize>,
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
    let (rule_sources, online) = if request.fetch_rules {
        rules::sources()
    } else {
        (local_rules::builtin_sources(), false)
    };
    if let Ok(mut progress) = phase.lock() {
        // `Some(0)` : catalogue intégré au moteur (hors ligne).
        progress.rules = Some(if online { rule_sources.len() } else { 0 });
    }

    set_phase(phase, Phase::Reading);
    let started = Instant::now();
    let mut files = collect::collect_files(request.path);
    let mut config = Config {
        rule_sources,
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
    let engine_request = AnalyzeRequest {
        files,
        git: workspace::git_context(request.path),
    };
    let response = runtime().block_on(herald_engine::analyze(&engine_request, &config));
    let duration = started.elapsed();

    Ok(Analysis {
        report: build_report(
            &engine_request.files,
            &response,
            request.previous_score,
            duration,
        ),
        dependencies: cve::dependencies(request.path, &engine_request.files),
    })
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

    AnalysisReport::new(
        Summary {
            score,
            previous_score,
            file_count: total_files as u32,
            language_count: language_count as u32,
            duration_ms: duration.as_millis().min(u128::from(u32::MAX)) as u32,
            issue_count: response.total_issues as u32,
            analyzed_at: "à l'instant".into(),
            analyzed_at_unix: cve::unix_now(),
        },
        categories,
        issues,
        sources,
    )
}

/// Extrait affichable d'un fichier.
fn source_file(path: &str, code: &str) -> SourceFile {
    SourceFile {
        path: path.to_owned().into(),
        first_line: 1,
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
        assert_eq!(progress.phase, Phase::Analyzing { files: 1 });
        assert_eq!(progress.files, vec!["src/config.js".to_owned()]);
    }
}
