//! Stockage local (SQLite) : projets, historique des analyses et corrections
//! marquées, dépendances, alertes CVE (et leur état lu), cache des fiches OSV.
//!
//! Base : `herald.db` dans le dossier de données de l'app (`paths::data_dir`, sur macOS
//! `~/Library/Application Support/fr.itsasync.herald.desktop`).
//! Rien n'en sort de la machine. Le code source n'y est pas copié : les extraits
//! affichés sont relus depuis le dossier du projet à l'ouverture.
//!
//! Toutes les fonctions sont tolérantes : si la base est inaccessible, l'app
//! fonctionne comme avant (en mémoire) et l'erreur est journalisée.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::engine::cve::{self, Dependency};
use crate::model::{Advisory, AnalysisReport, CategoryScore, Issue, Project, Summary};

/// Dossier de l'app, partagé avec la session et les dépôts clonés.
const DB_FILE: &str = "herald.db";
/// Analyses gardées par projet (les plus récentes).
const HISTORY_PER_PROJECT: i64 = 30;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
    path     TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    remote   TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS analyses (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_path TEXT NOT NULL,
    analyzed_at  INTEGER NOT NULL,
    score        INTEGER NOT NULL,
    issue_count  INTEGER NOT NULL,
    report       TEXT NOT NULL,
    resolved     TEXT NOT NULL DEFAULT '[]',
    remote_id    INTEGER
);
CREATE INDEX IF NOT EXISTS analyses_by_project ON analyses (project_path, id);
CREATE TABLE IF NOT EXISTS dependencies (
    project_path TEXT NOT NULL,
    ecosystem    TEXT NOT NULL,
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    PRIMARY KEY (project_path, ecosystem, name, version)
);
CREATE TABLE IF NOT EXISTS alerts (
    project_path      TEXT NOT NULL,
    osv_id            TEXT NOT NULL,
    package           TEXT NOT NULL,
    installed_version TEXT NOT NULL,
    data              TEXT NOT NULL,
    read              INTEGER NOT NULL DEFAULT 0,
    first_seen        INTEGER NOT NULL,
    PRIMARY KEY (project_path, osv_id, package, installed_version)
);
CREATE TABLE IF NOT EXISTS rule_overrides (
    id      TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS osv_cache (
    id         TEXT PRIMARY KEY,
    data       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);
";

/// Rapport sérialisé : tout sauf le code source.
#[derive(Serialize, Deserialize)]
struct StoredReport {
    summary: Summary,
    categories: Vec<CategoryScore>,
    issues: Vec<Issue>,
}

/// Une analyse de l'historique d'un projet (pas encore affiché par l'interface).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub analyzed_at: u64,
    pub score: u8,
    pub issue_count: u32,
}

fn db() -> Option<MutexGuard<'static, Connection>> {
    static DB: OnceLock<Option<Mutex<Connection>>> = OnceLock::new();
    DB.get_or_init(|| match open() {
        Ok(conn) => Some(Mutex::new(conn)),
        Err(err) => {
            eprintln!("[store] base locale indisponible : {err}");
            None
        }
    })
    .as_ref()
    .map(|db| db.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn open() -> Result<Connection, String> {
    let conn = if cfg!(test) {
        Connection::open_in_memory().map_err(|e| e.to_string())?
    } else {
        let dir = crate::paths::data_dir().ok_or("dossier de données introuvable")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Connection::open(dir.join(DB_FILE)).map_err(|e| e.to_string())?
    };
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    migrate(&conn);
    Ok(conn)
}

/// Colonnes ajoutées après la première version du schéma. Une colonne déjà
/// présente fait échouer `ALTER TABLE` : l'erreur est attendue et ignorée.
fn migrate(conn: &Connection) {
    const COLUMNS: &[&str] = &[
        // Dépendance déclarée par le projet (0 : transitive, lue dans un lockfile).
        "ALTER TABLE dependencies ADD COLUMN direct INTEGER NOT NULL DEFAULT 1",
        // Version exacte (lockfile ou version figée) ; 0 : borne basse d'une contrainte.
        "ALTER TABLE dependencies ADD COLUMN resolved INTEGER NOT NULL DEFAULT 0",
        // Fichier d'où vient la version.
        "ALTER TABLE dependencies ADD COLUMN source TEXT NOT NULL DEFAULT ''",
    ];
    for statement in COLUMNS {
        let _ = conn.execute(statement, []);
    }
}

fn log<T>(context: &str, result: rusqlite::Result<T>) -> Option<T> {
    result
        .map_err(|err| eprintln!("[store] {context} : {err}"))
        .ok()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// ── Projets ────────────────────────────────────────────────────────────────

/// Projets sauvegardés, avec leur dernier rapport ; `None` au tout premier
/// lancement (aucune sauvegarde encore), pour proposer des dossiers.
pub fn load_projects() -> Option<Vec<Project>> {
    let conn = db()?;
    let initialized: Option<String> = log(
        "lecture meta",
        conn.query_row(
            "SELECT value FROM meta WHERE key = 'initialized'",
            [],
            |r| r.get(0),
        )
        .optional(),
    )
    .flatten();
    initialized.as_ref()?;

    let mut stmt = log(
        "lecture projets",
        conn.prepare("SELECT path, name, remote FROM projects ORDER BY position, added_at"),
    )?;
    let rows: Vec<(String, String, Option<String>)> = log(
        "lecture projets",
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .and_then(Iterator::collect),
    )?;
    drop(stmt);

    let projects = rows
        .into_iter()
        .map(|(path, name, remote)| {
            let path = PathBuf::from(path);
            let report = latest_report(&conn, &path);
            Project {
                name: name.into(),
                path,
                remote,
                report,
            }
        })
        .collect();
    Some(projects)
}

fn latest_report(conn: &Connection, path: &Path) -> Option<AnalysisReport> {
    let (json, resolved): (String, String) = log(
        "lecture rapport",
        conn.query_row(
            "SELECT report, resolved FROM analyses WHERE project_path = ?1 ORDER BY id DESC LIMIT 1",
            params![path_key(path)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional(),
    )??;
    let stored: StoredReport = serde_json::from_str(&json)
        .map_err(|err| eprintln!("[store] rapport illisible ({}) : {err}", path.display()))
        .ok()?;
    let mut stored = stored;
    // Titres et « À revoir » recalculés : un rapport ancien profite des textes
    // français et de la mesure de précision à jour.
    for issue in &mut stored.issues {
        issue.title = crate::model::rule_title(&issue.rule).into();
        issue.review = crate::engine::local_rules::needs_review(&issue.rule);
    }
    let mut summary = stored.summary;
    summary.analyzed_at = analyzed_label(summary.analyzed_at_unix).into();
    let sources = crate::engine::read_sources(path, &stored.issues);
    let mut report = AnalysisReport::new(summary, stored.categories, stored.issues, sources);
    let resolved: Vec<usize> = serde_json::from_str(&resolved).unwrap_or_default();
    report.set_resolved(resolved);
    Some(report)
}

/// « il y a 5 min », « il y a 2 j »…
pub fn analyzed_label(at: u64) -> String {
    if at == 0 {
        return "lors d'une session précédente".to_owned();
    }
    crate::time::relative_label(at, cve::unix_now())
}

/// Enregistre la liste des projets, dans l'ordre affiché.
pub fn save_projects(projects: &[Project]) {
    let Some(mut conn) = db() else { return };
    let now = cve::unix_now() as i64;
    let Some(tx) = log("transaction projets", conn.transaction()) else {
        return;
    };
    let keep: HashSet<String> = projects.iter().map(|p| path_key(&p.path)).collect();
    for (position, project) in projects.iter().enumerate() {
        log(
            "enregistrement projet",
            tx.execute(
                "INSERT INTO projects (path, name, remote, position, added_at) VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT (path) DO UPDATE SET name = excluded.name, remote = excluded.remote, position = excluded.position",
                params![path_key(&project.path), project.name.as_ref(), project.remote, position as i64, now],
            ),
        );
    }
    // Projets retirés : on oublie aussi leurs analyses, dépendances et alertes.
    let existing: Vec<String> = tx
        .prepare("SELECT path FROM projects")
        .and_then(|mut stmt| stmt.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    for path in existing.into_iter().filter(|p| !keep.contains(p)) {
        for table in ["projects", "analyses", "dependencies", "alerts"] {
            let column = if table == "projects" {
                "path"
            } else {
                "project_path"
            };
            log(
                "suppression projet",
                tx.execute(
                    &format!("DELETE FROM {table} WHERE {column} = ?1"),
                    params![path],
                ),
            );
        }
    }
    log(
        "meta",
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('initialized', '1')",
            [],
        ),
    );
    log("validation projets", tx.commit());
}

// ── Analyses ───────────────────────────────────────────────────────────────

/// Ajoute une analyse à l'historique du projet ; renvoie son identifiant local.
pub fn save_analysis(path: &Path, report: &AnalysisReport) -> Option<i64> {
    let conn = db()?;
    let stored = StoredReport {
        summary: report.summary.clone(),
        categories: report.categories.clone(),
        issues: report.issues.clone(),
    };
    let json = serde_json::to_string(&stored).ok()?;
    let resolved = serde_json::to_string(&report.resolved_indices()).ok()?;
    let key = path_key(path);
    log(
        "enregistrement analyse",
        conn.execute(
            "INSERT INTO analyses (project_path, analyzed_at, score, issue_count, report, resolved) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                key,
                report.summary.analyzed_at_unix as i64,
                i64::from(report.summary.score),
                report.issues.len() as i64,
                json,
                resolved
            ],
        ),
    )?;
    let id = conn.last_insert_rowid();
    log(
        "purge historique",
        conn.execute(
            "DELETE FROM analyses WHERE project_path = ?1 AND id NOT IN \
             (SELECT id FROM analyses WHERE project_path = ?1 ORDER BY id DESC LIMIT ?2)",
            params![key, HISTORY_PER_PROJECT],
        ),
    );
    Some(id)
}

/// Met à jour les corrections marquées de la dernière analyse du projet.
pub fn save_resolved(path: &Path, report: &AnalysisReport) {
    let Some(conn) = db() else { return };
    let Ok(resolved) = serde_json::to_string(&report.resolved_indices()) else {
        return;
    };
    log(
        "corrections",
        conn.execute(
            "UPDATE analyses SET resolved = ?2 WHERE id = \
             (SELECT id FROM analyses WHERE project_path = ?1 ORDER BY id DESC LIMIT 1)",
            params![path_key(path), resolved],
        ),
    );
}

/// Historique des notes du projet, du plus récent au plus ancien.
#[allow(dead_code)]
pub fn history(path: &Path) -> Vec<HistoryEntry> {
    let Some(conn) = db() else {
        return Vec::new();
    };
    conn.prepare(
        "SELECT analyzed_at, score, issue_count FROM analyses WHERE project_path = ?1 ORDER BY id DESC",
    )
    .and_then(|mut stmt| {
        stmt.query_map(params![path_key(path)], |r| {
            Ok(HistoryEntry {
                analyzed_at: r.get::<_, i64>(0)?.max(0) as u64,
                score: r.get::<_, i64>(1)?.clamp(0, 100) as u8,
                issue_count: r.get::<_, i64>(2)?.max(0) as u32,
            })
        })?
        .collect()
    })
    .unwrap_or_default()
}

// ── Dépendances ────────────────────────────────────────────────────────────

pub fn save_dependencies(path: &Path, deps: &[Dependency]) {
    let Some(mut conn) = db() else { return };
    let Some(tx) = log("transaction dépendances", conn.transaction()) else {
        return;
    };
    let key = path_key(path);
    log(
        "dépendances",
        tx.execute(
            "DELETE FROM dependencies WHERE project_path = ?1",
            params![key],
        ),
    );
    for dep in deps {
        log(
            "dépendance",
            tx.execute(
                "INSERT OR IGNORE INTO dependencies (project_path, ecosystem, name, version, direct, resolved, source) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    key,
                    dep.ecosystem,
                    dep.name,
                    dep.version,
                    dep.direct,
                    dep.resolved,
                    dep.source
                ],
            ),
        );
    }
    log("validation dépendances", tx.commit());
}

/// Dépendances connues de chaque projet (revérification des CVE au démarrage,
/// onglet « Dépendances »), dans l'ordre d'enregistrement.
pub fn all_dependencies() -> HashMap<PathBuf, Vec<Dependency>> {
    let Some(conn) = db() else {
        return HashMap::new();
    };
    let rows: Vec<(String, Dependency)> = conn
        .prepare(
            "SELECT project_path, ecosystem, name, version, direct, resolved, source \
             FROM dependencies ORDER BY rowid",
        )
        .and_then(|mut stmt| {
            stmt.query_map([], |r| {
                Ok((
                    r.get(0)?,
                    Dependency {
                        ecosystem: r.get(1)?,
                        name: r.get(2)?,
                        version: r.get(3)?,
                        direct: r.get(4)?,
                        resolved: r.get(5)?,
                        source: r.get(6)?,
                    },
                ))
            })?
            .collect()
        })
        .unwrap_or_default();
    let mut out: HashMap<PathBuf, Vec<Dependency>> = HashMap::new();
    for (path, dep) in rows {
        out.entry(PathBuf::from(path)).or_default().push(dep);
    }
    out
}

// ── Alertes ────────────────────────────────────────────────────────────────

/// Remplace les alertes du projet par celles du dernier contrôle, en gardant
/// l'état « lue » des alertes déjà connues.
pub fn replace_alerts(path: &Path, advisories: &[Advisory]) {
    let Some(mut conn) = db() else { return };
    let Some(tx) = log("transaction alertes", conn.transaction()) else {
        return;
    };
    let key = path_key(path);
    let read: HashSet<(String, String, String)> = tx
        .prepare("SELECT osv_id, package, installed_version FROM alerts WHERE project_path = ?1 AND read = 1")
        .and_then(|mut stmt| {
            stmt.query_map(params![key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect()
        })
        .unwrap_or_default();
    let first_seen: HashMap<(String, String, String), i64> = tx
        .prepare("SELECT osv_id, package, installed_version, first_seen FROM alerts WHERE project_path = ?1")
        .and_then(|mut stmt| {
            stmt.query_map(params![key], |r| {
                Ok(((r.get(0)?, r.get(1)?, r.get(2)?), r.get(3)?))
            })?
            .collect()
        })
        .unwrap_or_default();
    log(
        "alertes",
        tx.execute("DELETE FROM alerts WHERE project_path = ?1", params![key]),
    );
    let now = cve::unix_now() as i64;
    for advisory in advisories {
        let id = (
            advisory.osv_id.to_string(),
            advisory.package.to_string(),
            advisory.installed_version.to_string(),
        );
        let mut advisory = advisory.clone();
        advisory.read = read.contains(&id);
        let Ok(data) = serde_json::to_string(&advisory) else {
            continue;
        };
        log(
            "alerte",
            tx.execute(
                "INSERT OR REPLACE INTO alerts (project_path, osv_id, package, installed_version, data, read, first_seen) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    key,
                    id.0,
                    id.1,
                    id.2,
                    data,
                    advisory.read,
                    first_seen.get(&id).copied().unwrap_or(now)
                ],
            ),
        );
    }
    log("validation alertes", tx.commit());
}

/// Toutes les alertes, les plus graves puis les plus récentes d'abord.
pub fn load_alerts() -> Vec<Advisory> {
    let Some(conn) = db() else {
        return Vec::new();
    };
    let rows: Vec<(String, bool)> = conn
        .prepare("SELECT data, read FROM alerts")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .unwrap_or_default();
    let mut alerts: Vec<Advisory> = rows
        .into_iter()
        .filter_map(|(data, read)| {
            let mut advisory: Advisory = serde_json::from_str(&data).ok()?;
            advisory.read = read;
            Some(advisory)
        })
        .collect();
    alerts.sort_by(|a, b| {
        a.read
            .cmp(&b.read)
            .then(a.severity.cmp(&b.severity))
            .then(b.published_unix.cmp(&a.published_unix))
    });
    alerts
}

pub fn set_alert_read(advisory: &Advisory) {
    let Some(conn) = db() else { return };
    log(
        "alerte lue",
        conn.execute(
            "UPDATE alerts SET read = 1 WHERE project_path = ?1 AND osv_id = ?2 AND package = ?3 AND installed_version = ?4",
            params![
                path_key(&advisory.project_path),
                advisory.osv_id.as_ref(),
                advisory.package.as_ref(),
                advisory.installed_version.as_ref()
            ],
        ),
    );
}

pub fn set_all_alerts_read() {
    let Some(conn) = db() else { return };
    log(
        "alertes lues",
        conn.execute("UPDATE alerts SET read = 1", []),
    );
}

// ── Règles ─────────────────────────────────────────────────────────────────

/// Choix de l'utilisateur sur l'écran Règles : règle → activée ou non. Une
/// règle absente suit l'état par défaut du catalogue.
pub fn rule_overrides() -> HashMap<String, bool> {
    let Some(conn) = db() else {
        return HashMap::new();
    };
    conn.prepare("SELECT id, enabled FROM rule_overrides")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .unwrap_or_default()
}

/// Enregistre le choix pour une règle ; `None` revient à l'état par défaut.
pub fn set_rule_override(id: &str, enabled: Option<bool>) {
    let Some(conn) = db() else { return };
    let result = match enabled {
        Some(enabled) => conn.execute(
            "INSERT OR REPLACE INTO rule_overrides (id, enabled) VALUES (?1, ?2)",
            params![id, enabled],
        ),
        None => conn.execute("DELETE FROM rule_overrides WHERE id = ?1", params![id]),
    };
    log("règle", result);
}

/// Revient à l'état par défaut pour toutes les règles.
pub fn clear_rule_overrides() {
    let Some(conn) = db() else { return };
    log("règles", conn.execute("DELETE FROM rule_overrides", []));
}

// ── Réglages et données locales ────────────────────────────────────────────

/// Réglage booléen local (`meta`), `default` s'il n'a jamais été changé.
pub fn setting(key: &str, default: bool) -> bool {
    let Some(conn) = db() else { return default };
    log(
        "réglage",
        conn.query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![format!("setting.{key}")],
            |r| r.get::<_, String>(0),
        )
        .optional(),
    )
    .flatten()
    .map_or(default, |value| value == "1")
}

pub fn set_setting(key: &str, value: bool) {
    let Some(conn) = db() else { return };
    log(
        "réglage",
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![format!("setting.{key}"), if value { "1" } else { "0" }],
        ),
    );
}

/// Dossier des données de l'app (base, dépôts clonés, session de développement).
pub fn data_dir() -> Option<PathBuf> {
    crate::paths::data_dir()
}

/// Taille de la base locale, en octets.
pub fn database_size() -> u64 {
    let Some(dir) = data_dir() else { return 0 };
    ["herald.db", "herald.db-wal", "herald.db-shm"]
        .iter()
        .filter_map(|name| std::fs::metadata(dir.join(name)).ok())
        .map(|meta| meta.len())
        .sum()
}

/// Efface toutes les données locales : projets, analyses, alertes, cache OSV et
/// choix de règles. Les dossiers des projets ne sont pas touchés.
pub fn clear_local_data() {
    let Some(conn) = db() else { return };
    log(
        "effacement",
        conn.execute_batch(
            "DELETE FROM projects; DELETE FROM analyses; DELETE FROM dependencies; \
             DELETE FROM alerts; DELETE FROM osv_cache; DELETE FROM rule_overrides; \
             VACUUM;",
        ),
    );
}

// ── Cache OSV ──────────────────────────────────────────────────────────────

/// Fiche OSV en cache et date de récupération.
pub fn osv_cached(id: &str) -> Option<(String, u64)> {
    let conn = db()?;
    log(
        "cache OSV",
        conn.query_row(
            "SELECT data, fetched_at FROM osv_cache WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)?.max(0) as u64)),
        )
        .optional(),
    )?
}

pub fn osv_cache_put(id: &str, data: &str, fetched_at: u64) {
    let Some(conn) = db() else { return };
    log(
        "cache OSV",
        conn.execute(
            "INSERT OR REPLACE INTO osv_cache (id, data, fetched_at) VALUES (?1, ?2, ?3)",
            params![id, data, fetched_at as i64],
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Severity};

    fn report(score: u8, at: u64) -> AnalysisReport {
        AnalysisReport::new(
            Summary {
                score,
                previous_score: None,
                file_count: 3,
                language_count: 1,
                duration_ms: 10,
                issue_count: 1,
                analyzed_at: "à l'instant".into(),
                analyzed_at_unix: at,
            },
            vec![],
            vec![Issue {
                rule: "eval-usage".into(),
                title: "Eval usage".into(),
                severity: Severity::Critical,
                category: Category::Security,
                file: "src/a.js".into(),
                line: Some(1),
                explanation: None,
                fix: None,
                references: None,
                review: false,
            }],
            vec![],
        )
    }

    /// Un seul test : la base en mémoire est partagée par tout le binaire de test.
    #[test]
    fn projects_analyses_and_alerts_round_trip() {
        let path = PathBuf::from("/tmp/herald-store-test");
        let project = Project::from_path(path.clone());
        save_projects(std::slice::from_ref(&project));

        save_analysis(&path, &report(40, 100));
        let mut latest = report(55, 200);
        latest.toggle_group_resolved(0);
        save_analysis(&path, &latest);
        save_resolved(&path, &latest);

        let projects = load_projects().expect("initialisée");
        assert_eq!(projects.len(), 1);
        let restored = projects[0].report.as_ref().expect("rapport");
        assert_eq!(restored.summary.score, 55);
        assert!(restored.is_resolved(0), "correction marquée conservée");
        assert_eq!(history(&path).len(), 2);

        let mut alert = crate::model::advisory_for_tests(Some("4.21.1"));
        alert.project_path = path.clone();
        replace_alerts(&path, std::slice::from_ref(&alert));
        set_alert_read(&alert);
        replace_alerts(&path, std::slice::from_ref(&alert));
        let alerts = load_alerts();
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].read, "l'état lu survit à un nouveau contrôle");

        set_rule_override("eval-usage", Some(false));
        set_rule_override("long-line", Some(true));
        set_rule_override("long-line", None);
        assert_eq!(
            rule_overrides(),
            HashMap::from([("eval-usage".to_owned(), false)])
        );
        clear_rule_overrides();
        assert!(rule_overrides().is_empty());

        assert!(setting("cve_on_start", true));
        set_setting("cve_on_start", false);
        assert!(!setting("cve_on_start", true));

        osv_cache_put("GHSA-1", "{}", 42);
        assert_eq!(osv_cached("GHSA-1"), Some(("{}".to_owned(), 42)));

        save_projects(&[]);
        assert!(load_projects().unwrap().is_empty());
        assert!(history(&path).is_empty());
        assert!(load_alerts().is_empty());
    }
}
