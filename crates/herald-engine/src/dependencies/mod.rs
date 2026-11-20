pub mod osv;
pub mod registry;
pub mod manifest;

use std::time::Duration;

use futures::stream::{self, StreamExt};

use crate::category::Category;
use crate::types::{Issue, Severity as IssueSeverity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Cargo,
    Pip,
    Go,
}

impl PackageManager {
    pub fn osv_ecosystem(self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Cargo => "crates.io",
            PackageManager::Pip => "PyPI",
            PackageManager::Go => "Go",
        }
    }

    /// Page de documentation du paquet (registre de l'écosystème).
    pub fn doc_url(self, name: &str) -> String {
        match self {
            PackageManager::Npm => format!("https://www.npmjs.com/package/{name}"),
            PackageManager::Cargo => format!("https://crates.io/crates/{name}"),
            PackageManager::Pip => format!("https://pypi.org/project/{name}"),
            PackageManager::Go => format!("https://pkg.go.dev/{name}"),
        }
    }
}

/// URL de l'advisory (GitHub pour les GHSA, OSV sinon).
fn advisory_url(id: &str) -> String {
    if id.starts_with("GHSA") {
        format!("https://github.com/advisories/{id}")
    } else {
        format!("https://osv.dev/vulnerability/{id}")
    }
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub manager: PackageManager,
    pub name: String,
    pub version_spec: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct Advisory {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub dep: Dependency,
    pub latest: Option<String>,
    pub is_outdated: bool,
    pub advisories: Vec<Advisory>,
}

const CONCURRENCY: usize = 10;

/// Budget global pour TOUT le manifest (toutes dépendances confondues). Un
/// registre/OSV lent retimeout à 5s par appel, mais avec beaucoup de
/// dépendances (buffer_unordered à 10 en vol) ça peut quand même s'accumuler
/// — mieux vaut renvoyer les statuts déjà obtenus que de laisser tout le
/// rapport bloqué en attendant les derniers.
const CHECK_ALL_BUDGET: Duration = Duration::from_secs(20);

pub async fn check_all(deps: Vec<Dependency>) -> Vec<DependencyStatus> {
    let client = reqwest::Client::builder()
        .user_agent("herald/0.1 (dependency-checker)")
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let checks = stream::iter(deps).map(|dep| check_one(&client, dep)).buffer_unordered(CONCURRENCY);

    tokio::time::timeout(CHECK_ALL_BUDGET, checks.collect()).await.unwrap_or_default()
}

async fn check_one(client: &reqwest::Client, dep: Dependency) -> DependencyStatus {
    let current = clean_version(&dep.version_spec);

    let (latest, advisories) = tokio::join!(
        registry::latest(client, dep.manager, &dep.name),
        osv::query(client, &dep.name, &current, dep.manager.osv_ecosystem()),
    );

    DependencyStatus {
        is_outdated: is_outdated(&current, latest.as_deref()),
        dep,
        latest,
        advisories,
    }
}

fn clean_version(spec: &str) -> String {
    spec.trim_start_matches(|c: char| !c.is_ascii_digit()).to_string()
}

fn is_outdated(current: &str, latest: Option<&str>) -> bool {
    match (
        semver::Version::parse(current).ok(),
        latest.and_then(|l| semver::Version::parse(l).ok()),
    ) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

fn sev_rank(s: Severity) -> u8 {
    match s {
        Severity::Unknown => 0,
        Severity::Low => 1,
        Severity::Medium => 2,
        Severity::High => 3,
        Severity::Critical => 4,
    }
}

/// UN seul relevé par dépendance à problème (pas de doublon) : la vulnérabilité
/// la plus grave si elle est vulnérable, sinon « obsolète ». Les dépendances
/// saines (à jour, sans advisory) ne produisent rien.
pub fn to_issues(statuses: &[DependencyStatus]) -> Vec<Issue> {
    let mut issues = Vec::new();

    for s in statuses {
        if let Some(worst) = s.advisories.iter().max_by_key(|a| sev_rank(a.severity)) {
            let extra = s.advisories.len() - 1;
            let suffix = match extra {
                0 => String::new(),
                1 => " (+1 autre)".to_string(),
                n => format!(" (+{n} autres)"),
            };
            // Suffixes parsables côté front : « · maj: <version> » + « · doc: <url> ».
            let maj = s
                .latest
                .as_deref()
                .map(|l| format!(" · maj: {l}"))
                .unwrap_or_default();
            issues.push(Issue {
                rule: "vulnerable-dependency".into(),
                category: Category::Dependencies,
                severity: map_severity(worst.severity),
                message: format!(
                    "{} ({}): {} [{}]{}{} · doc: {}",
                    s.dep.name,
                    s.dep.version_spec,
                    worst.summary,
                    worst.id,
                    suffix,
                    maj,
                    advisory_url(&worst.id)
                ),
                line: 0,
                column: 0,
            });
        } else if s.is_outdated {
            issues.push(Issue {
                rule: "outdated-dependency".into(),
                category: Category::Dependencies,
                severity: IssueSeverity::Info,
                message: format!(
                    "{} ({}) obsolète · maj: {} · doc: {}",
                    s.dep.name,
                    s.dep.version_spec,
                    s.latest.as_deref().unwrap_or("?"),
                    s.dep.manager.doc_url(&s.dep.name)
                ),
                line: 0,
                column: 0,
            });
        }
    }

    issues
}

/// Version non figée (`"*"`, `"latest"`…) : le paquet réellement installé peut
/// changer d'une build à l'autre sans qu'un humain le décide — un risque de
/// chaîne d'approvisionnement distinct d'`outdated-dependency`, qui suppose
/// au contraire une version figée mais dépassée. Vérif purement locale (pas
/// de réseau), donc indépendante de `check_all`/`to_issues`.
pub fn to_wildcard_issues(deps: &[Dependency]) -> Vec<Issue> {
    deps.iter()
        .filter(|d| is_wildcard_spec(&d.version_spec))
        .map(|d| Issue {
            rule: "unpinned-dependency-wildcard".into(),
            category: Category::Dependencies,
            severity: IssueSeverity::Warning,
            message: format!(
                "{} : version non figée ({}) — épingle une version précise pour des builds reproductibles",
                d.name, d.version_spec
            ),
            line: 0,
            column: 0,
        })
        .collect()
}

/// Même dépendance déclarée avec des contraintes de version différentes dans
/// plusieurs manifests d'un même dépôt (monorepo) — un `npm install` à un
/// endroit et un autre ailleurs peuvent résoudre deux versions différentes du
/// même paquet. Vérif purement locale (texte des specs), pas de réseau.
///
/// Limité à Npm/Pip : Cargo (et Go) résolvent TOUJOURS une version unique
/// pour tout le workspace via un lockfile partagé (Cargo.lock/go.sum), donc
/// des contraintes différentes par crate/module n'y sont pas un vrai risque —
/// juste du bruit (37 faux positifs sur le seul workspace ripgrep).
pub fn to_duplicate_version_issues(all: &[(usize, Dependency)]) -> Vec<(usize, Issue)> {
    use std::collections::HashMap;
    let mut by_name: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (i, d) in all {
        if !matches!(d.manager, PackageManager::Npm | PackageManager::Pip) {
            continue;
        }
        by_name.entry(d.name.as_str()).or_default().push((*i, d.version_spec.as_str()));
    }

    let mut out = Vec::new();
    for (name, occurrences) in by_name {
        let mut distinct: Vec<&str> = occurrences.iter().map(|(_, v)| *v).collect();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() < 2 {
            continue;
        }
        let versions = distinct.join(", ");
        for (i, _) in &occurrences {
            out.push((
                *i,
                Issue {
                    rule: "duplicate-dependency-version".into(),
                    category: Category::Dependencies,
                    severity: IssueSeverity::Warning,
                    message: format!(
                        "{name} : versions divergentes entre manifests ({versions}) — un même paquet peut résoudre différemment selon l'endroit du monorepo, aligne les contraintes"
                    ),
                    line: 0,
                    column: 0,
                },
            ));
        }
    }
    out
}

fn is_wildcard_spec(spec: &str) -> bool {
    let s = spec.trim().trim_start_matches(['^', '~', '=']).trim();
    s.eq_ignore_ascii_case("*") || s.eq_ignore_ascii_case("latest") || s == "x" || s == "x.x.x"
}

fn map_severity(sev: Severity) -> IssueSeverity {
    match sev {
        Severity::Critical | Severity::High => IssueSeverity::Error,
        Severity::Medium => IssueSeverity::Warning,
        Severity::Low | Severity::Unknown => IssueSeverity::Info,
    }
}