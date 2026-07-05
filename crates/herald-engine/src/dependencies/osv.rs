use serde::Deserialize;

use crate::dependencies::{Advisory, Severity};

const OSV_URL: &str = "https://api.osv.dev/v1/query";

#[derive(serde::Serialize)]
struct Query<'a> {
    version: &'a str,
    package: Package<'a>,
}
#[derive(serde::Serialize)]
struct Package<'a> {
    name: &'a str,
    ecosystem: &'a str,
}

#[derive(Deserialize)]
struct OsvResponse {
    #[serde(default)]
    vulns: Vec<Vuln>,
}
#[derive(Deserialize)]
struct Vuln {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    database_specific: DatabaseSpecific,
}
#[derive(Deserialize, Default)]
struct DatabaseSpecific {
    #[serde(default)]
    severity: String,
}

pub async fn query(
    client: &reqwest::Client,
    name: &str,
    version: &str,
    ecosystem: &str,
) -> Vec<Advisory> {
    let body = Query { version, package: Package { name, ecosystem } };

    let resp = match client.post(OSV_URL).json(&body).send().await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let parsed = match resp.json::<OsvResponse>().await {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    parsed.vulns.into_iter().map(|v| Advisory {
        id: v.id,
        severity: parse_severity(&v.database_specific.severity),
        summary: v.summary,
    }).collect()
}

fn parse_severity(s: &str) -> Severity {
    match s.to_ascii_uppercase().as_str() {
        "LOW" => Severity::Low,
        "MODERATE" | "MEDIUM" => Severity::Medium,
        "HIGH" => Severity::High,
        "CRITICAL" => Severity::Critical,
        _ => Severity::Unknown,
    }
}