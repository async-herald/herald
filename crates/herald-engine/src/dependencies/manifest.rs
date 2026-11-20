use std::collections::BTreeMap;

use serde::Deserialize;

use crate::dependencies::{Dependency, PackageManager};

const PARSERS: &[(&str, fn(&str) -> Vec<Dependency>)] = &[
    ("package.json", npm),
    ("Cargo.toml", cargo),
    ("requirements.txt", pip),
    ("go.mod", go),
];

fn filename_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn parse(path: &str, code: &str) -> Option<Vec<Dependency>> {
    let name = filename_of(path);
    PARSERS.iter().find(|p| p.0 == name).map(|p| (p.1)(code))
}

pub fn is_manifest(path: &str) -> bool {
    let name = filename_of(path);
    PARSERS.iter().any(|p| p.0 == name)
}

fn dep(manager: PackageManager, name: impl Into<String>, version_spec: impl Into<String>) -> Dependency {
    Dependency {
        manager,
        name: name.into(),
        version_spec: version_spec.into(),
    }
}

#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
}

fn npm(code: &str) -> Vec<Dependency> {
    let pkg: PackageJson = match serde_json::from_str(code) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    pkg.dependencies
        .into_iter()
        .chain(pkg.dev_dependencies)
        .map(|(name, version)| dep(PackageManager::Npm, name, version))
        .collect()
}

#[derive(Deserialize)]
struct CargoToml {
    #[serde(default)]
    dependencies: BTreeMap<String, CargoDep>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<String, CargoDep>,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: BTreeMap<String, CargoDep>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CargoDep {
    Simple(String),
    Detailed { version: Option<String> },
}

impl CargoDep {
    fn version(self) -> Option<String> {
        match self {
            CargoDep::Simple(v) => Some(v),
            CargoDep::Detailed { version } => version,
        }
    }
}

fn cargo(code: &str) -> Vec<Dependency> {
    let manifest: CargoToml = match toml::from_str(code) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    manifest
        .dependencies
        .into_iter()
        .chain(manifest.dev_dependencies)
        .chain(manifest.build_dependencies)
        .filter_map(|(name, d)| d.version().map(|v| dep(PackageManager::Cargo, name, v)))
        .collect()
}

fn pip(code: &str) -> Vec<Dependency> {
    code.lines()
        .filter_map(parse_pip_line)
        .map(|(name, version)| dep(PackageManager::Pip, name, version))
        .collect()
}

fn parse_pip_line(line: &str) -> Option<(String, String)> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() || line.starts_with('-') || line.contains("://") {
        return None;
    }

    let op = line.find(['=', '<', '>', '!', '~'])?;
    let name = line[..op].split('[').next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }

    Some((name.to_string(), line[op..].trim().to_string()))
}

fn go(code: &str) -> Vec<Dependency> {
    let mut deps = Vec::new();
    let mut in_block = false;

    for raw in code.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if in_block {
            if line == ")" {
                in_block = false;
            } else if let Some(d) = parse_go_require(line) {
                deps.push(d);
            }
        } else if line == "require (" {
            in_block = true;
        } else if let Some(rest) = line.strip_prefix("require ")
            && let Some(d) = parse_go_require(rest.trim()) {
                deps.push(d);
            }
    }

    deps
}

fn parse_go_require(line: &str) -> Option<Dependency> {
    let mut parts = line.split_whitespace();
    let name = parts.next()?;
    let version = parts.next()?;
    Some(dep(PackageManager::Go, name, version))
}
