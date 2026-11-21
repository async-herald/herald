//! Ligne de commande, sans fenêtre : analyse pour l'extension VS Code.
//!
//! Contrat complet dans `herald-vscode/DESKTOP-CLI.md` :
//! - `herald-desktop --analyze <dossier> --json` : même analyse que dans la
//!   fenêtre (mêmes règles et réglages, `herald.config.json`, historique, CVE).
//!   Progression sur stderr, une ligne
//!   `PROGRESS <pourcentage> <message>` par étape ; à la fin, un seul objet JSON
//!   sur stdout (le rapport, ou `{ "error", "code" }` avec un code de sortie 1).
//! - `herald-desktop --open <dossier>` : ouvre la fenêtre sur ce projet.
//! - `herald-desktop --version` : affiche la version.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::engine::{self, Phase, SharedPhase, cve};
use crate::model::{AnalysisReport, Category, Project, Severity};
use crate::store;

/// Format du rapport JSON ; l'extension refuse un format plus récent.
const REPORT_VERSION: u32 = 1;

/// Ce que demande la ligne de commande.
pub enum Command {
    Analyze(PathBuf),
    Open(PathBuf),
    Version,
    /// Lancement normal (aucune option reconnue).
    Window,
}

/// Lit les arguments (sans le nom du programme). Les arguments inconnus
/// (par exemple `-psn_…` ajouté par macOS) sont ignorés.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Command {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--analyze") => {
                return args.next().map_or(Command::Analyze(PathBuf::new()), |dir| {
                    Command::Analyze(PathBuf::from(dir))
                });
            }
            Some("--open") => {
                if let Some(dir) = args.next() {
                    return Command::Open(PathBuf::from(dir));
                }
            }
            Some("--version") => return Command::Version,
            _ => {}
        }
    }
    Command::Window
}

static OPEN: OnceLock<PathBuf> = OnceLock::new();

/// Dossier demandé par `--open`, lu par la fenêtre au démarrage.
pub fn set_open(path: PathBuf) {
    let path = path.canonicalize().unwrap_or(path);
    OPEN.set(path).ok();
}

pub fn requested_open() -> Option<PathBuf> {
    OPEN.get().filter(|path| path.is_dir()).cloned()
}

/// Erreur renvoyée à l'extension : code stable et message lisible.
struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// `--analyze` : analyse, écrit le rapport sur stdout ; renvoie le code de sortie.
pub fn run_analyze(path: &Path) -> i32 {
    progress(0, "Préparation");
    let (output, code) = match analyze(path) {
        Ok(report) => (report, 0),
        Err(failure) => (json!({ "error": failure.message, "code": failure.code }), 1),
    };
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{output}").ok();
    stdout.flush().ok();
    code
}

fn analyze(path: &Path) -> Result<Value, Failure> {
    if path.as_os_str().is_empty() {
        return Err(Failure::new(
            "invalid_path",
            "Aucun dossier indiqué après --analyze.",
        ));
    }
    let path = path
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
        .ok_or_else(|| {
            Failure::new(
                "invalid_path",
                format!("Dossier introuvable : {}", path.display()),
            )
        })?;

    // Projet de l'app (ajouté s'il n'y est pas) : dépôt distant, note précédente.
    let mut projects = store::load_projects().unwrap_or_default();
    let ix = match projects.iter().position(|project| project.path == path) {
        Some(ix) => ix,
        None => {
            projects.push(Project::from_path(path.clone()));
            store::save_projects(&projects);
            projects.len() - 1
        }
    };
    let project = &projects[ix];
    let name = project.name.to_string();
    let remote = project.remote.clone();
    let previous_score = project.report.as_ref().map(|report| report.summary.score);

    let phase = SharedPhase::default();
    let done = Arc::new(AtomicBool::new(false));
    let reporter = {
        let phase = Arc::clone(&phase);
        let done = Arc::clone(&done);
        std::thread::spawn(move || report_progress(&phase, &done))
    };
    let result = engine::analyze_project(
        engine::Request {
            path: &path,
            remote: remote.as_deref(),
            fetch_rules: true,
            previous_score,
        },
        &phase,
    );
    done.store(true, Ordering::Relaxed);
    reporter.join().ok();
    let analysis = result.map_err(|message| {
        let code = if message.starts_with("Aucun fichier") {
            "no_files"
        } else {
            "analysis"
        };
        Failure::new(code, message)
    })?;
    let report = analysis.report;

    // Historique local, comme une analyse lancée depuis la fenêtre.
    progress(95, "Enregistrement dans l'historique");
    let local_id = store::save_analysis(&path, &report);
    store::save_dependencies(&path, &analysis.dependencies);
    let _ = local_id;

    progress(97, "Vérification des dépendances (CVE)");
    match cve::scan(&name, &path, &analysis.dependencies) {
        Ok(advisories) => store::replace_alerts(&path, &advisories),
        Err(err) => eprintln!("[cve] {err}"),
    }
    progress(100, "Analyse terminée");

    Ok(report_json(&name, &path, &report))
}

/// Écrit l'étape en cours tant que l'analyse tourne (une ligne par changement).
fn report_progress(phase: &SharedPhase, done: &AtomicBool) {
    let mut last = None;
    while !done.load(Ordering::Relaxed) {
        let current = phase.lock().ok().map(|progress| {
            let (percent, message) = match &progress.phase {
                Phase::Preparing => (2, "Préparation du projet".to_owned()),
                Phase::Rules => (8, "Chargement des règles".to_owned()),
                Phase::Reading => (15, "Lecture des fichiers".to_owned()),
                Phase::Analyzing { files } => {
                    let total = (*files).max(1);
                    let ratio = progress.done.min(total) as f32 / total as f32;
                    (
                        20 + (ratio * 65.) as u32,
                        format!(
                            "Analyse des fichiers · {} / {files}",
                            progress.done.min(total)
                        ),
                    )
                }
                Phase::Dependencies => (88, "Relevé des dépendances".to_owned()),
                Phase::Scoring => (92, "Calcul de la note".to_owned()),
            };
            (percent, message)
        });
        if current.is_some() && current != last {
            if let Some((percent, message)) = &current {
                progress(*percent, message);
            }
            last = current;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn progress(percent: u32, message: &str) {
    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "PROGRESS {percent} {message}").ok();
    stderr.flush().ok();
}

fn report_json(name: &str, path: &Path, report: &AnalysisReport) -> Value {
    let summary = &report.summary;
    let categories: Vec<Value> = report
        .categories
        .iter()
        .map(|category| {
            json!({
                "id": category_id(category.category),
                "label": category.category.label(),
                "score": category.score,
                "grade": category.grade().label(),
                "applicable": category.applicable,
            })
        })
        .collect();
    let issues: Vec<Value> = report
        .issues
        .iter()
        // « À revoir » : hors note, gardé pour l'app.
        .filter(|issue| !issue.review)
        .map(|issue| {
            let mut value = json!({
                "rule": issue.rule.as_ref(),
                "title": issue.title.as_ref(),
                "severity": severity_id(issue.severity),
                "category": category_id(issue.category),
                "file": issue.file.as_ref(),
            });
            if let Some(line) = issue.line {
                value["line"] = json!(line);
            }
            if let Some(explanation) = &issue.explanation {
                value["explanation"] = json!(explanation.as_ref());
            }
            if let Some(fix) = &issue.fix {
                value["fix"] = json!(fix.as_ref());
            }
            value
        })
        .collect();
    let mut value = json!({
        "version": REPORT_VERSION,
        "project": name,
        "path": path.display().to_string(),
        "analyzedAt": iso8601(summary.analyzed_at_unix),
        "durationMs": summary.duration_ms,
        "score": summary.score,
        "grade": summary.grade().label(),
        "files": summary.file_count,
        "categories": categories,
        "issues": issues,
    });
    if let Some(previous) = summary.previous_score {
        value["previousScore"] = json!(previous);
    }
    value
}

fn category_id(category: Category) -> &'static str {
    match category {
        Category::Security => "security",
        Category::Quality => "quality",
        Category::Git => "git",
        Category::Devops => "devops",
        Category::Pattern => "pattern",
        Category::Dependencies => "dependencies",
        Category::Tests => "tests",
    }
}

fn severity_id(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

/// Date UTC au format ISO 8601 (`2026-11-21T14:02:00Z`).
fn iso8601(unix: u64) -> String {
    let unix = if unix == 0 { cve::unix_now() } else { unix };
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Jours depuis 1970 → date civile (algorithme de H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn lit_les_options() {
        assert!(matches!(
            parse(args(&["--analyze", "/tmp/p", "--json"])),
            Command::Analyze(path) if path == Path::new("/tmp/p")
        ));
        assert!(matches!(
            parse(args(&["--open", "/tmp/p"])),
            Command::Open(_)
        ));
        assert!(matches!(parse(args(&["--version"])), Command::Version));
        assert!(matches!(parse(args(&["-psn_0_123"])), Command::Window));
        assert!(matches!(parse(args(&[])), Command::Window));
    }

    #[test]
    fn date_iso() {
        assert_eq!(iso8601(0).len(), 20);
        assert_eq!(iso8601(1_795_269_720), "2026-11-21T14:02:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
    }
}
