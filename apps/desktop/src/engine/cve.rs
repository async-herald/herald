//! Alertes CVE réelles : dépendances du projet (versions installées lues dans
//! les lockfiles, sinon déclarées dans les manifestes), croisées avec la base
//! OSV (osv.dev, qui agrège GitHub Advisory, PyPA, RustSec, Go, Packagist…).
//!
//! Comme pour l'analyse (et comme le backend), seuls les noms et versions des
//! dépendances sont envoyés à OSV : aucun fichier ni chemin. Les fiches des
//! vulnérabilités sont gardées en cache local (`store`) 24 h.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use herald_engine::dependencies::{Dependency as EngineDependency, manifest};
use herald_engine::types::FileInput;
use serde::{Deserialize, Serialize};

use super::lockfile;
use crate::model::{Advisory, Severity};
use crate::store;

const OSV_API: &str = "https://api.osv.dev/v1";
/// OSV accepte 1 000 requêtes par lot.
const BATCH_SIZE: usize = 1_000;
const TIMEOUT: Duration = Duration::from_secs(20);
/// Durée de validité d'une fiche en cache.
const CACHE_TTL_SECS: u64 = 24 * 3_600;

/// Dépendance du projet, avec la version vérifiée auprès d'OSV.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Dependency {
    /// Écosystème au sens OSV (`npm`, `crates.io`, `PyPI`, `Go`, `Packagist`).
    pub ecosystem: String,
    pub name: String,
    /// Version installée (lockfile) ou, à défaut, déclarée (`^4.18.2` → `4.18.2`).
    pub version: String,
    /// Déclarée par le projet (sinon : tirée par une autre dépendance).
    pub direct: bool,
    /// Version exacte : lue dans un lockfile ou figée dans le manifeste
    /// (`==2.31.0`). Sinon c'est la borne basse d'une contrainte (`^4.18.2`) :
    /// la version installée peut être plus récente, une faille est à confirmer.
    pub resolved: bool,
    /// Fichier d'où vient la version (`package-lock.json`, `api/Cargo.toml`).
    pub source: String,
}

/// Dépendances du projet, directes et transitives, sans doublon.
///
/// Les versions viennent des lockfiles quand il y en a (version réellement
/// installée), sinon des manifestes. `files` sont les fichiers retenus pour
/// l'analyse ; les lockfiles, qui n'en font pas partie, sont relus depuis `root`.
pub fn dependencies(root: &Path, files: &[FileInput]) -> Vec<Dependency> {
    let tree = super::collect::project_files(root);
    let read = |relative: &str| -> Option<String> {
        let full = root.join(relative);
        let size = std::fs::metadata(&full).ok()?.len();
        if size > lockfile::MAX_LOCKFILE_BYTES {
            return None;
        }
        std::fs::read_to_string(full).ok()
    };

    // Noms déclarés par les manifestes : distinguent les dépendances directes.
    let manifests: Vec<(&str, Vec<EngineDependency>)> = files
        .iter()
        .filter_map(|file| Some((file.path.as_str(), manifest::parse(&file.path, &file.code)?)))
        .collect();
    let mut declared: HashSet<(String, String)> = manifests
        .iter()
        .flat_map(|(_, deps)| deps)
        .map(|dep| {
            let ecosystem = dep.manager.osv_ecosystem();
            (
                ecosystem.to_owned(),
                lockfile::normalized(ecosystem, &dep.name),
            )
        })
        .collect();
    for path in tree.iter().filter(|path| lockfile::is_declaration(path)) {
        if let Some(text) = read(path) {
            declared.extend(
                lockfile::declared_names(path, &text)
                    .into_iter()
                    .map(|(eco, name)| (eco.to_owned(), lockfile::normalized(eco, &name))),
            );
        }
    }

    let mut out = Deduped::default();

    // 1. Lockfiles : versions installées, directes et transitives.
    let mut locked: HashSet<(String, String)> = HashSet::new();
    for path in tree.iter().filter(|path| lockfile::is_lockfile(path)) {
        let Some(found) = read(path).and_then(|text| lockfile::parse(path, &text)) else {
            continue;
        };
        for entry in found {
            let key = (
                entry.ecosystem.to_owned(),
                lockfile::normalized(entry.ecosystem, &entry.name),
            );
            let direct = entry.direct.unwrap_or_else(|| declared.contains(&key));
            locked.insert(key);
            out.push(Dependency {
                ecosystem: entry.ecosystem.to_owned(),
                name: entry.name,
                version: entry.version,
                direct,
                resolved: true,
                source: path.clone(),
            });
        }
    }

    // 2. Manifestes : seulement ce qu'aucun lockfile ne couvre.
    for (path, deps) in manifests {
        for dep in deps {
            let ecosystem = dep.manager.osv_ecosystem();
            if locked.contains(&(
                ecosystem.to_owned(),
                lockfile::normalized(ecosystem, &dep.name),
            )) {
                continue;
            }
            let Some((version, resolved)) = declared_version(ecosystem, &dep.version_spec) else {
                continue;
            };
            out.push(Dependency {
                ecosystem: ecosystem.to_owned(),
                name: dep.name,
                version,
                direct: true,
                resolved,
                source: path.to_owned(),
            });
        }
    }
    out.list
}

/// Liste sans doublon (écosystème, nom, version) ; un paquet à la fois direct
/// et transitif reste direct.
#[derive(Default)]
struct Deduped {
    list: Vec<Dependency>,
    index: HashMap<(String, String, String), usize>,
}

impl Deduped {
    fn push(&mut self, dep: Dependency) {
        let key = (dep.ecosystem.clone(), dep.name.clone(), dep.version.clone());
        match self.index.get(&key) {
            Some(&ix) => self.list[ix].direct |= dep.direct,
            None => {
                self.index.insert(key, self.list.len());
                self.list.push(dep);
            }
        }
    }
}

/// Version à vérifier pour une contrainte de manifeste, et si elle est exacte.
///
/// `None` quand la contrainte ne dit rien de la version installée (`*`,
/// `latest`, `>=2.0`, `<3`) : la vérifier produirait des faux positifs.
fn declared_version(ecosystem: &str, spec: &str) -> Option<(String, bool)> {
    let spec = spec.trim();
    let version = clean_version(spec)?;
    let exact = match ecosystem {
        // pip : seul `==` fige la version ; `~=1.4` en donne la borne basse.
        "PyPI" => {
            if spec.starts_with("~=") && !spec.contains(',') {
                return Some((version, false));
            }
            if !spec.starts_with("==") || spec.contains(['*', ',']) {
                return None;
            }
            true
        }
        // go.mod : toujours une version exacte.
        "Go" => true,
        // npm / Cargo : une borne haute ou une union ne donne pas la version installée.
        _ => {
            if spec.starts_with(['>', '<']) || spec.contains("||") || spec.contains(' ') {
                return None;
            }
            // Version nue : exacte en npm, compatible (`^`) en Cargo.
            match ecosystem {
                "npm" => spec.starts_with('=') || spec.starts_with(|c: char| c.is_ascii_digit()),
                _ => spec.starts_with('='),
            }
        }
    };
    Some((version, exact))
}

/// `^4.18.2` → `4.18.2` ; `None` pour une version non figée (`*`, `latest`, plage).
fn clean_version(spec: &str) -> Option<String> {
    let version = spec
        .trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split([' ', ',', '|'])
        .next()
        .unwrap_or_default()
        .trim();
    let valid = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        && !version.contains(['x', 'X'])
        && version.chars().next().is_some_and(|c| c.is_ascii_digit());
    valid.then(|| version.to_owned())
}

// ── API OSV ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct BatchQuery<'a> {
    queries: Vec<Query<'a>>,
}

#[derive(Serialize)]
struct Query<'a> {
    package: QueryPackage<'a>,
    version: &'a str,
}

#[derive(Serialize)]
struct QueryPackage<'a> {
    name: &'a str,
    ecosystem: &'a str,
}

#[derive(Deserialize)]
struct BatchResponse {
    #[serde(default)]
    results: Vec<BatchResult>,
}

#[derive(Deserialize, Default)]
struct BatchResult {
    #[serde(default)]
    vulns: Vec<VulnRef>,
}

#[derive(Deserialize)]
struct VulnRef {
    id: String,
}

/// Fiche OSV (seuls les champs utiles).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Vuln {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    published: String,
    /// Date de retrait : l'avis a été invalidé (doublon, erreur), à ignorer.
    #[serde(default)]
    withdrawn: Option<String>,
    #[serde(default)]
    severity: Vec<VulnSeverity>,
    #[serde(default)]
    affected: Vec<Affected>,
    #[serde(default)]
    database_specific: Option<DatabaseSpecific>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VulnSeverity {
    #[serde(rename = "type")]
    kind: String,
    score: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct DatabaseSpecific {
    #[serde(default)]
    severity: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Affected {
    #[serde(default)]
    package: Option<AffectedPackage>,
    #[serde(default)]
    ranges: Vec<Range>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AffectedPackage {
    #[serde(default)]
    name: String,
    #[serde(default)]
    ecosystem: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Range {
    #[serde(default)]
    events: Vec<Event>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Event {
    #[serde(default)]
    fixed: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(true)
        .build()
        .into()
}

/// Identifiants des vulnérabilités qui touchent chaque dépendance (même ordre).
fn query_batch(agent: &ureq::Agent, deps: &[Dependency]) -> Result<Vec<Vec<String>>, String> {
    let mut out = Vec::with_capacity(deps.len());
    for chunk in deps.chunks(BATCH_SIZE) {
        let body = BatchQuery {
            queries: chunk
                .iter()
                .map(|dep| Query {
                    package: QueryPackage {
                        name: &dep.name,
                        ecosystem: &dep.ecosystem,
                    },
                    version: &dep.version,
                })
                .collect(),
        };
        let response: BatchResponse = agent
            .post(format!("{OSV_API}/querybatch"))
            .send_json(&body)
            .map_err(|_| "Base OSV injoignable.".to_owned())?
            .into_body()
            .read_json()
            .map_err(|_| "Réponse OSV invalide.".to_owned())?;
        let mut results = response.results.into_iter();
        for _ in chunk {
            let ids = results
                .next()
                .unwrap_or_default()
                .vulns
                .into_iter()
                .map(|vuln| vuln.id)
                .collect();
            out.push(ids);
        }
    }
    Ok(out)
}

/// Fiche d'une vulnérabilité, depuis le cache local ou OSV.
fn vuln(agent: &ureq::Agent, id: &str, now: u64) -> Option<Vuln> {
    if let Some((json, fetched_at)) = store::osv_cached(id) {
        if now.saturating_sub(fetched_at) < CACHE_TTL_SECS {
            if let Ok(vuln) = serde_json::from_str(&json) {
                return Some(vuln);
            }
        }
    }
    let fetched: Option<Vuln> = agent
        .get(format!("{OSV_API}/vulns/{id}"))
        .call()
        .ok()
        .and_then(|response| response.into_body().read_json().ok());
    match fetched {
        Some(vuln) => {
            if let Ok(json) = serde_json::to_string(&vuln) {
                store::osv_cache_put(id, &json, now);
            }
            Some(vuln)
        }
        // Hors ligne : une fiche périmée vaut mieux que rien.
        None => store::osv_cached(id).and_then(|(json, _)| serde_json::from_str(&json).ok()),
    }
}

/// Vulnérabilités connues des dépendances du projet. Bloquant (réseau).
pub fn scan(
    project_name: &str,
    project_path: &Path,
    deps: &[Dependency],
) -> Result<Vec<Advisory>, String> {
    if deps.is_empty() {
        return Ok(Vec::new());
    }
    let agent = agent();
    let now = unix_now();
    let hits = query_batch(&agent, deps)?;

    let mut vulns: HashMap<String, Option<Vuln>> = HashMap::new();
    let mut advisories = Vec::new();
    for (dep, ids) in deps.iter().zip(hits) {
        for id in ids {
            let vuln = vulns
                .entry(id.clone())
                .or_insert_with(|| vuln(&agent, &id, now));
            let Some(vuln) = vuln else { continue };
            if vuln.withdrawn.is_some() {
                continue;
            }
            advisories.push(to_advisory(vuln, dep, project_name, project_path));
        }
    }
    advisories.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(b.cvss.unwrap_or(0.).total_cmp(&a.cvss.unwrap_or(0.)))
            .then(b.published_unix.cmp(&a.published_unix))
    });
    Ok(advisories)
}

fn to_advisory(vuln: &Vuln, dep: &Dependency, project_name: &str, project_path: &Path) -> Advisory {
    let cvss = vuln
        .severity
        .iter()
        .filter(|s| s.kind == "CVSS_V3")
        .find_map(|s| cvss3_base_score(&s.score));
    let level = vuln
        .database_specific
        .as_ref()
        .and_then(|d| d.severity.as_deref())
        .map(str::to_ascii_uppercase);
    // Base OSV des paquets malveillants (vers npm, typosquatting) : pas de
    // niveau publié, mais le paquet est à retirer quoi qu'il arrive.
    let malicious = std::iter::once(&vuln.id)
        .chain(&vuln.aliases)
        .any(|id| id.starts_with("MAL-"));
    let severity = match (level.as_deref(), cvss) {
        _ if malicious => Severity::Critical,
        (Some("CRITICAL" | "HIGH"), _) => Severity::Critical,
        (Some("MODERATE" | "MEDIUM"), _) => Severity::Warning,
        (Some("LOW"), _) => Severity::Info,
        (_, Some(score)) if score >= 7.0 => Severity::Critical,
        (_, Some(score)) if score >= 4.0 => Severity::Warning,
        (_, Some(_)) => Severity::Info,
        // Sans niveau publié, une vulnérabilité connue reste à regarder.
        (_, None) => Severity::Warning,
    };
    let display_id = vuln
        .aliases
        .iter()
        .find(|alias| alias.starts_with("CVE-"))
        .cloned()
        .unwrap_or_else(|| vuln.id.clone());
    let summary = if vuln.summary.trim().is_empty() {
        first_sentence(&vuln.details)
    } else {
        vuln.summary.trim().to_owned()
    };
    let summary = if malicious {
        format!("Paquet malveillant, à retirer et à considérer comme compromis : {summary}")
    } else {
        summary
    };
    let url = if vuln.id.starts_with("GHSA") {
        format!("https://github.com/advisories/{}", vuln.id)
    } else {
        format!("https://osv.dev/vulnerability/{}", vuln.id)
    };

    Advisory {
        id: display_id.into(),
        osv_id: vuln.id.clone().into(),
        summary: summary.into(),
        severity,
        cvss,
        ecosystem: dep.ecosystem.clone().into(),
        package: dep.name.clone().into(),
        installed_version: dep.version.clone().into(),
        fixed_version: fixed_version(vuln, dep).map(SharedString::from),
        project: project_name.to_owned().into(),
        project_path: project_path.to_path_buf(),
        url: url.into(),
        published_unix: parse_rfc3339_date(&vuln.published).unwrap_or(0),
        read: false,
        confirmed: dep.resolved,
        direct: dep.direct,
    }
}

/// La plus haute des versions (comparaison semver, sinon texte) : la version
/// qui corrige toutes les failles d'un paquet.
pub fn highest_version<'a>(versions: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    versions
        .into_iter()
        .max_by(|a, b| match (semver_lenient(a), semver_lenient(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            _ => a.cmp(b),
        })
}

fn first_sentence(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    let line = line.unwrap_or("Vulnérabilité publiée");
    let mut out: String = line.chars().take(180).collect();
    if line.chars().count() > 180 {
        out.push('…');
    }
    out
}

/// Plus petite version corrigée au-dessus de la version installée.
fn fixed_version(vuln: &Vuln, dep: &Dependency) -> Option<String> {
    let same_package = |affected: &&Affected| {
        affected.package.as_ref().is_some_and(|p| {
            p.ecosystem.eq_ignore_ascii_case(&dep.ecosystem)
                && p.name.eq_ignore_ascii_case(&dep.name)
        })
    };
    let fixes: Vec<&str> = vuln
        .affected
        .iter()
        .filter(same_package)
        .flat_map(|affected| &affected.ranges)
        .flat_map(|range| &range.events)
        .filter_map(|event| event.fixed.as_deref())
        .collect();
    let installed = semver_lenient(&dep.version);
    let mut candidates: Vec<(semver::Version, &str)> = fixes
        .iter()
        .filter_map(|fix| semver_lenient(fix).map(|v| (v, *fix)))
        .filter(|(v, _)| installed.as_ref().is_none_or(|i| v > i))
        .collect();
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates
        .first()
        .map(|(_, fix)| (*fix).to_owned())
        .or_else(|| fixes.first().map(|fix| (*fix).to_owned()))
}

/// `1.2` → `1.2.0`, `v1.2.3` → `1.2.3`.
fn semver_lenient(version: &str) -> Option<semver::Version> {
    let version = version.trim_start_matches('v');
    semver::Version::parse(version).ok().or_else(|| {
        let parts: Vec<&str> = version.split('.').collect();
        match parts.as_slice() {
            [major] => semver::Version::parse(&format!("{major}.0.0")).ok(),
            [major, minor] => semver::Version::parse(&format!("{major}.{minor}.0")).ok(),
            _ => None,
        }
    })
}

// ── CVSS v3 ────────────────────────────────────────────────────────────────

/// Score de base CVSS 3.0 / 3.1 à partir du vecteur (`CVSS:3.1/AV:N/…`).
pub fn cvss3_base_score(vector: &str) -> Option<f32> {
    let metrics: HashMap<&str, &str> = vector
        .split('/')
        .skip(1)
        .filter_map(|part| part.split_once(':'))
        .collect();
    if !vector.starts_with("CVSS:3") {
        return None;
    }
    let scope_changed = match *metrics.get("S")? {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    let av = match *metrics.get("AV")? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let ac = match *metrics.get("AC")? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let pr = match (*metrics.get("PR")?, scope_changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let ui = match *metrics.get("UI")? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let cia = |key: &str| -> Option<f64> {
        match *metrics.get(key)? {
            "H" => Some(0.56),
            "L" => Some(0.22),
            "N" => Some(0.),
            _ => None,
        }
    };
    let (c, i, a) = (cia("C")?, cia("I")?, cia("A")?);

    let iss = 1. - (1. - c) * (1. - i) * (1. - a);
    let impact = if scope_changed {
        7.52 * (iss - 0.029) - 3.25 * (iss - 0.02).powi(15)
    } else {
        6.42 * iss
    };
    let exploitability = 8.22 * av * ac * pr * ui;
    let score = if impact <= 0. {
        0.
    } else if scope_changed {
        roundup((1.08 * (impact + exploitability)).min(10.))
    } else {
        roundup((impact + exploitability).min(10.))
    };
    Some(score as f32)
}

/// Arrondi supérieur au dixième, tel que défini par la spécification CVSS 3.1.
fn roundup(value: f64) -> f64 {
    let int_input = (value * 100_000.).round() as i64;
    if int_input % 10_000 == 0 {
        int_input as f64 / 100_000.
    } else {
        ((int_input / 10_000) + 1) as f64 / 10.
    }
}

// ── Dates ──────────────────────────────────────────────────────────────────

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `2024-03-12T15:30:00Z` → secondes Unix (à la journée près suffit).
fn parse_rfc3339_date(text: &str) -> Option<u64> {
    let date = text.get(..10)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Jours depuis 1970-01-01 (algorithme « days from civil » de H. Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400).ok()
}

/// « il y a 3 j » jusqu'à une semaine, puis la date (« 12/03/2024 »).
pub fn published_label(at: u64, now: u64) -> String {
    if at == 0 {
        return "date inconnue".to_owned();
    }
    if now.saturating_sub(at) < 7 * 86_400 {
        return crate::time::relative_label(at, now);
    }
    format!("le {}", date_label(at))
}

/// Date seule : « 12/03/2024 ».
pub fn date_label(at: u64) -> String {
    let days = (at / 86_400) as i64;
    // Inverse de `parse_rfc3339_date` (« civil from days »).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day:02}/{month:02}/{year}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvss_vectors_match_the_specification() {
        assert_eq!(
            cvss3_base_score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"),
            Some(9.8)
        );
        assert_eq!(
            cvss3_base_score("CVSS:3.1/AV:N/AC:L/PR:N/UI:R/S:C/C:L/I:L/A:N"),
            Some(6.1)
        );
        assert_eq!(
            cvss3_base_score("CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:N/A:H"),
            Some(5.9)
        );
        assert_eq!(cvss3_base_score("CVSS:4.0/AV:N"), None);
    }

    #[test]
    fn versions_are_cleaned() {
        assert_eq!(clean_version("^4.18.2").as_deref(), Some("4.18.2"));
        assert_eq!(clean_version("==2.31.0").as_deref(), Some("2.31.0"));
        assert_eq!(clean_version("v1.9.1").as_deref(), Some("1.9.1"));
        assert_eq!(clean_version("*"), None);
        assert_eq!(clean_version("latest"), None);
        assert_eq!(clean_version("1.x"), None);
    }

    #[test]
    fn dates_round_trip() {
        let at = parse_rfc3339_date("2024-03-12T15:30:00Z").unwrap();
        assert_eq!(at, 1_710_201_600);
        assert_eq!(published_label(at, at + 30 * 86_400), "le 12/03/2024");
    }

    fn project(name: &str, files: &[(&str, &str)]) -> (std::path::PathBuf, Vec<FileInput>) {
        let root = std::env::temp_dir().join(format!("herald-deps-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let mut inputs = Vec::new();
        for (path, code) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, code).unwrap();
            if manifest::is_manifest(path) {
                inputs.push(FileInput {
                    path: (*path).into(),
                    code: (*code).into(),
                });
            }
        }
        (root, inputs)
    }

    #[test]
    fn manifest_alone_gives_declared_versions_to_confirm() {
        let (root, files) = project(
            "manifest",
            &[(
                "package.json",
                r#"{"dependencies":{"lodash":"^4.17.15","left-pad":"*","react":"18.2.0","old":">=1.0.0"}}"#,
            )],
        );
        let deps = dependencies(&root, &files);
        std::fs::remove_dir_all(&root).ok();
        let summary: Vec<(&str, &str, bool)> = deps
            .iter()
            .map(|d| (d.name.as_str(), d.version.as_str(), d.resolved))
            .collect();
        assert_eq!(
            summary,
            vec![("lodash", "4.17.15", false), ("react", "18.2.0", true)]
        );
        assert!(deps.iter().all(|d| d.direct && d.source == "package.json"));
    }

    #[test]
    fn lockfile_wins_over_the_manifest_and_adds_transitives() {
        let (root, files) = project(
            "lock",
            &[
                ("package.json", r#"{"dependencies":{"lodash":"^4.17.15"}}"#),
                (
                    "package-lock.json",
                    r#"{"lockfileVersion":3,"packages":{
                        "":{"dependencies":{"lodash":"^4.17.15"}},
                        "node_modules/lodash":{"version":"4.17.21"},
                        "node_modules/minimist":{"version":"1.2.5"}}}"#,
                ),
                ("api/requirements.txt", "requests>=2.0\ndjango==4.2.1\n"),
            ],
        );
        let deps = dependencies(&root, &files);
        std::fs::remove_dir_all(&root).ok();
        let summary: Vec<(&str, &str, bool, bool)> = deps
            .iter()
            .map(|d| (d.name.as_str(), d.version.as_str(), d.direct, d.resolved))
            .collect();
        assert_eq!(
            summary,
            vec![
                // Version installée, pas la borne basse du manifeste.
                ("lodash", "4.17.21", true, true),
                ("minimist", "1.2.5", false, true),
                // `requests>=2.0` ne dit rien de la version installée : écarté.
                ("django", "4.2.1", true, true),
            ]
        );
    }

    #[test]
    fn declared_versions_by_ecosystem() {
        assert_eq!(
            declared_version("PyPI", "==2.31.0"),
            Some(("2.31.0".into(), true))
        );
        assert_eq!(
            declared_version("PyPI", "~=1.4"),
            Some(("1.4".into(), false))
        );
        assert_eq!(declared_version("PyPI", ">=2.0"), None);
        assert_eq!(
            declared_version("crates.io", "1.0"),
            Some(("1.0".into(), false))
        );
        assert_eq!(
            declared_version("crates.io", "=1.0.3"),
            Some(("1.0.3".into(), true))
        );
        assert_eq!(
            declared_version("npm", "~1.2.3"),
            Some(("1.2.3".into(), false))
        );
        assert_eq!(declared_version("npm", "1.2.3 - 2.0.0"), None);
        assert_eq!(
            declared_version("Go", "v1.9.1"),
            Some(("1.9.1".into(), true))
        );
    }

    #[test]
    fn malicious_packages_are_critical() {
        let vuln: Vuln = serde_json::from_str(
            r#"{"id":"MAL-2025-1234","summary":"Malicious code in evil-pkg (npm)"}"#,
        )
        .unwrap();
        let advisory = to_advisory(&vuln, &lodash("1.0.0"), "demo", Path::new("/tmp/demo"));
        assert_eq!(advisory.severity, Severity::Critical);
        assert!(advisory.summary.starts_with("Paquet malveillant"));
    }

    #[test]
    fn highest_version_compares_semver() {
        assert_eq!(
            highest_version(["4.17.12", "4.17.21", "4.9.0"]),
            Some("4.17.21")
        );
    }

    #[test]
    fn fixed_version_is_the_next_one_above_installed() {
        let vuln: Vuln = serde_json::from_str(
            r#"{"id":"GHSA-1","affected":[{"package":{"name":"lodash","ecosystem":"npm"},
                "ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"4.17.21"}]},
                          {"type":"SEMVER","events":[{"introduced":"3.0.0"},{"fixed":"4.17.12"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            fixed_version(&vuln, &lodash("4.17.15")).as_deref(),
            Some("4.17.21")
        );
    }

    fn lodash(version: &str) -> Dependency {
        Dependency {
            ecosystem: "npm".into(),
            name: "lodash".into(),
            version: version.into(),
            direct: true,
            resolved: true,
            source: "package-lock.json".into(),
        }
    }

    /// Réseau : vérifie la vraie base OSV (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn lodash_4_17_15_is_vulnerable_on_osv() {
        let deps = vec![lodash("4.17.15")];
        let advisories = scan("t", Path::new("/tmp/t"), &deps).unwrap();
        assert!(!advisories.is_empty());
        assert!(advisories.iter().any(|a| a.id.starts_with("CVE-")));
    }
}
