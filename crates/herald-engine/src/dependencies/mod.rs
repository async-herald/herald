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

pub async fn check_all(deps: Vec<Dependency>) -> Vec<DependencyStatus> {
    let client = reqwest::Client::builder()
        .user_agent("herald/0.1 (dependency-checker)")
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    stream::iter(deps)
        .map(|dep| check_one(&client, dep))
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await
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

fn map_severity(sev: Severity) -> IssueSeverity {
    match sev {
        Severity::Critical | Severity::High => IssueSeverity::Error,
        Severity::Medium => IssueSeverity::Warning,
        Severity::Low | Severity::Unknown => IssueSeverity::Info,
    }
}