//! Versions réellement installées, lues dans les lockfiles du projet.
//!
//! Un manifeste ne donne qu'une contrainte (`^4.17.15`, `>=2.0`) : interroger OSV
//! avec la borne basse signale des failles corrigées depuis longtemps dans la
//! version installée (faux positif), et ignore toutes les dépendances
//! transitives (faux négatif). Le lockfile donne la version exacte de chaque
//! paquet, direct ou transitif.
//!
//! Formats lus : `package-lock.json` / `npm-shrinkwrap.json` (v1 à v3),
//! `yarn.lock` (v1 et Berry), `pnpm-lock.yaml` (v5 à v9), `Cargo.lock`,
//! `poetry.lock`, `uv.lock`, `Pipfile.lock`, `composer.lock` et `go.mod`
//! (versions exactes, `// indirect` pour les transitives).

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

/// Taille maximale d'un lockfile lu (un `package-lock.json` de monorepo dépasse
/// facilement les 500 Ko des fichiers de code).
pub const MAX_LOCKFILE_BYTES: u64 = 40 * 1024 * 1024;

/// Paquet à version exacte trouvé dans un lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locked {
    /// Écosystème au sens OSV (`npm`, `crates.io`, `PyPI`, `Go`, `Packagist`).
    pub ecosystem: &'static str,
    pub name: String,
    pub version: String,
    /// Déclaré directement par le projet ; `None` quand le lockfile ne le dit
    /// pas (on regarde alors les manifestes).
    pub direct: Option<bool>,
}

type Parser = fn(&str) -> Vec<Locked>;

const LOCKFILES: &[(&str, Parser)] = &[
    ("package-lock.json", npm_lock),
    ("npm-shrinkwrap.json", npm_lock),
    ("yarn.lock", yarn_lock),
    ("pnpm-lock.yaml", pnpm_lock),
    ("Cargo.lock", cargo_lock),
    ("poetry.lock", python_toml_lock),
    ("uv.lock", python_toml_lock),
    ("Pipfile.lock", pipfile_lock),
    ("composer.lock", composer_lock),
    ("go.mod", go_mod),
];

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn is_lockfile(path: &str) -> bool {
    let name = file_name(path);
    LOCKFILES.iter().any(|(file, _)| *file == name)
}

/// Paquets d'un lockfile ; `None` si le fichier n'en est pas un.
pub fn parse(path: &str, text: &str) -> Option<Vec<Locked>> {
    let name = file_name(path);
    LOCKFILES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, parse)| parse(text))
}

/// Une version exploitable par OSV : commence par un chiffre, sans URL ni chemin.
fn exact(version: &str) -> Option<String> {
    let version = version
        .trim()
        .trim_matches('"')
        .trim_start_matches(['v', '=']);
    let ok = version.chars().next().is_some_and(|c| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
    ok.then(|| version.to_owned())
}

fn locked(
    ecosystem: &'static str,
    name: &str,
    version: &str,
    direct: Option<bool>,
) -> Option<Locked> {
    let name = name.trim();
    (!name.is_empty()).then_some(())?;
    Some(Locked {
        ecosystem,
        name: name.to_owned(),
        version: exact(version)?,
        direct,
    })
}

// ── npm ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NpmLock {
    /// v2 / v3 : `"node_modules/a/node_modules/b": { "version": … }`.
    #[serde(default)]
    packages: BTreeMap<String, NpmPackage>,
    /// v1 : arbre imbriqué.
    #[serde(default)]
    dependencies: BTreeMap<String, NpmV1Dependency>,
}

#[derive(Deserialize)]
struct NpmPackage {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    link: bool,
    #[serde(default)]
    dependencies: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct NpmV1Dependency {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, NpmV1Dependency>,
}

fn npm_lock(text: &str) -> Vec<Locked> {
    let Ok(lock) = serde_json::from_str::<NpmLock>(text) else {
        return Vec::new();
    };
    if lock.packages.is_empty() {
        let mut out = Vec::new();
        npm_v1(&lock.dependencies, true, &mut out);
        return out;
    }
    // Dépendances directes : celles du projet racine et des workspaces
    // (entrées hors `node_modules/`).
    let direct: HashSet<&str> = lock
        .packages
        .iter()
        .filter(|(key, _)| !key.contains("node_modules/"))
        .flat_map(|(_, package)| {
            package
                .dependencies
                .keys()
                .chain(package.dev_dependencies.keys())
                .chain(package.optional_dependencies.keys())
        })
        .map(String::as_str)
        .collect();
    lock.packages
        .iter()
        .filter(|(_, package)| !package.link)
        .filter_map(|(key, package)| {
            let (_, installed_as) = key.rsplit_once("node_modules/")?;
            // Alias (`"foo": "npm:bar@1.0.0"`) : le vrai paquet est `name`.
            let name = package.name.as_deref().unwrap_or(installed_as);
            // Seul l'exemplaire hissé à la racine est une dépendance directe.
            let top_level = key.matches("node_modules/").count() == 1;
            let is_direct = top_level && direct.contains(installed_as);
            locked("npm", name, package.version.as_deref()?, Some(is_direct))
        })
        .collect()
}

fn npm_v1(deps: &BTreeMap<String, NpmV1Dependency>, top_level: bool, out: &mut Vec<Locked>) {
    for (name, dep) in deps {
        // v1 ne distingue pas direct / transitif à la racine : les manifestes tranchent.
        let direct = (!top_level).then_some(false);
        if let Some(found) = dep
            .version
            .as_deref()
            .and_then(|version| locked("npm", name, version, direct))
        {
            out.push(found);
        }
        npm_v1(&dep.dependencies, false, out);
    }
}

/// Nom d'un paquet dans un spécificateur `nom@contrainte` (`@scope/nom@^1`).
fn npm_name_of_spec(spec: &str) -> Option<&str> {
    let spec = spec.trim().trim_matches('"');
    let at = spec[1..].find('@')? + 1;
    Some(&spec[..at])
}

fn yarn_lock(text: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            current = None;
            let header = line.trim_end_matches(':');
            let first = header
                .split(", ")
                .next()
                .unwrap_or_default()
                .trim_matches('"');
            // Paquets du dépôt lui-même (workspaces, liens) : pas sur npm.
            let local = ["@workspace:", "@link:", "@portal:", "@file:"]
                .iter()
                .any(|marker| first.contains(marker));
            if !local && first != "__metadata" {
                current = npm_name_of_spec(first).map(str::to_owned);
            }
            continue;
        }
        let Some(name) = &current else { continue };
        let trimmed = line.trim();
        // v1 : `version "1.2.3"` ; Berry : `version: 1.2.3`.
        let version = trimmed
            .strip_prefix("version ")
            .or_else(|| trimmed.strip_prefix("version: "));
        if let Some(found) = version.and_then(|version| locked("npm", name, version, None)) {
            out.push(found);
            current = None;
        }
    }
    out
}

fn pnpm_lock(text: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut in_packages = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_packages = line.trim_end() == "packages:";
            continue;
        }
        // Clés de paquet : exactement deux espaces d'indentation.
        if !in_packages || !line.starts_with("  ") || line.starts_with("   ") {
            continue;
        }
        let key = line.trim().trim_end_matches(':').trim_matches(['\'', '"']);
        let key = key.strip_prefix('/').unwrap_or(key);
        // Suffixe de peers v6+ : `(react@18.2.0)`.
        let key = key.split('(').next().unwrap_or(key);
        let Some((name, version)) = pnpm_name_version(key) else {
            continue;
        };
        if let Some(found) = locked("npm", name, version, None) {
            out.push(found);
        }
    }
    out
}

/// `nom@1.2.3`, `@scope/nom@1.2.3` (v6+) ou `nom/1.2.3`, `@scope/nom/1.2.3_peer@2` (v5).
fn pnpm_name_version(key: &str) -> Option<(&str, &str)> {
    // Le `@` d'un scope est en tête : on cherche le séparateur après lui.
    let scope = usize::from(key.starts_with('@'));
    let rest = key.get(scope..)?;
    let slash_after_scope = if scope == 1 {
        rest.find('/')? + scope + 1
    } else {
        0
    };
    let tail = &key[slash_after_scope..];
    if let Some(at) = tail.find('@') {
        let split = slash_after_scope + at;
        let (name, version) = (&key[..split], &key[split + 1..]);
        // v5 avec peer (`nom/1.0.0_peer@2`) : le `@` est celui du peer.
        if !name.contains('/') || (scope == 1 && name.matches('/').count() == 1) {
            return Some((name, version));
        }
    }
    let (name, version) = key.rsplit_once('/')?;
    Some((name, version.split('_').next().unwrap_or(version)))
}

// ── Rust ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TomlLock {
    #[serde(default)]
    package: Vec<toml::Value>,
}

fn cargo_lock(text: &str) -> Vec<Locked> {
    let Ok(lock) = toml::from_str::<TomlLock>(text) else {
        return Vec::new();
    };
    lock.package
        .iter()
        .filter_map(|package| {
            // Sans `source` : crate du workspace ; `git+…` : pas sur crates.io.
            let source = package.get("source")?.as_str()?;
            if !(source.starts_with("registry+") || source.starts_with("sparse+")) {
                return None;
            }
            locked(
                "crates.io",
                package.get("name")?.as_str()?,
                package.get("version")?.as_str()?,
                None,
            )
        })
        .collect()
}

// ── Python ─────────────────────────────────────────────────────────────────

/// `poetry.lock` et `uv.lock` : `[[package]]` avec `name` et `version`.
fn python_toml_lock(text: &str) -> Vec<Locked> {
    let Ok(lock) = toml::from_str::<TomlLock>(text) else {
        return Vec::new();
    };
    lock.package
        .iter()
        .filter_map(|package| {
            // Paquet local, éditable ou tiré d'un dépôt Git : pas sur PyPI.
            if let Some(source) = package.get("source") {
                let is_local = match source {
                    toml::Value::Table(table) => {
                        ["editable", "virtual", "directory", "path", "git", "url"]
                            .iter()
                            .any(|key| table.contains_key(*key))
                            || table
                                .get("type")
                                .and_then(toml::Value::as_str)
                                .is_some_and(|kind| kind != "legacy")
                    }
                    _ => false,
                };
                if is_local {
                    return None;
                }
            }
            locked(
                "PyPI",
                package.get("name")?.as_str()?,
                package.get("version")?.as_str()?,
                None,
            )
        })
        .collect()
}

#[derive(Deserialize)]
struct PipfileLock {
    #[serde(default)]
    default: BTreeMap<String, PipfileEntry>,
    #[serde(default)]
    develop: BTreeMap<String, PipfileEntry>,
}

#[derive(Deserialize)]
struct PipfileEntry {
    #[serde(default)]
    version: Option<String>,
}

fn pipfile_lock(text: &str) -> Vec<Locked> {
    let Ok(lock) = serde_json::from_str::<PipfileLock>(text) else {
        return Vec::new();
    };
    lock.default
        .iter()
        .chain(&lock.develop)
        .filter_map(|(name, entry)| locked("PyPI", name, entry.version.as_deref()?, None))
        .collect()
}

// ── PHP ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ComposerLock {
    #[serde(default)]
    packages: Vec<ComposerPackage>,
    #[serde(default, rename = "packages-dev")]
    packages_dev: Vec<ComposerPackage>,
}

#[derive(Deserialize)]
struct ComposerPackage {
    name: String,
    version: String,
}

fn composer_lock(text: &str) -> Vec<Locked> {
    let Ok(lock) = serde_json::from_str::<ComposerLock>(text) else {
        return Vec::new();
    };
    lock.packages
        .iter()
        .chain(&lock.packages_dev)
        .filter_map(|package| locked("Packagist", &package.name, &package.version, None))
        .collect()
}

// ── Go ─────────────────────────────────────────────────────────────────────

/// `go.mod` liste toutes les versions retenues depuis Go 1.17, transitives
/// comprises (marquées `// indirect`).
fn go_mod(text: &str) -> Vec<Locked> {
    let mut out = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let indirect = raw.contains("// indirect");
        let line = raw.split("//").next().unwrap_or_default().trim();
        let require = if in_block {
            if line == ")" {
                in_block = false;
                continue;
            }
            line
        } else if line == "require (" {
            in_block = true;
            continue;
        } else if let Some(rest) = line.strip_prefix("require ") {
            rest.trim()
        } else {
            continue;
        };
        let mut parts = require.split_whitespace();
        if let (Some(name), Some(version)) = (parts.next(), parts.next()) {
            if let Some(found) = locked("Go", name, version, Some(!indirect)) {
                out.push(found);
            }
        }
    }
    out
}

// ── Dépendances directes hors moteur ───────────────────────────────────────

/// Noms déclarés par `pyproject.toml` (PEP 621 et Poetry) et `composer.json`,
/// que le moteur ne lit pas : ils servent à distinguer les directes.
pub fn declared_names(path: &str, text: &str) -> Vec<(&'static str, String)> {
    match file_name(path) {
        "pyproject.toml" => {
            let Ok(doc) = toml::from_str::<toml::Value>(text) else {
                return Vec::new();
            };
            let pep621 = doc
                .get("project")
                .and_then(|project| project.get("dependencies"))
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
                .map(|requirement| {
                    requirement
                        .split(|c: char| {
                            !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                        })
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                });
            let poetry = doc
                .get("tool")
                .and_then(|tool| tool.get("poetry"))
                .into_iter()
                .flat_map(|poetry| {
                    ["dependencies", "dev-dependencies"]
                        .into_iter()
                        .filter_map(move |key| poetry.get(key)?.as_table())
                        .flat_map(|table| table.keys().cloned())
                })
                .filter(|name| name != "python");
            pep621
                .chain(poetry)
                .filter(|name| !name.is_empty())
                .map(|name| ("PyPI", name))
                .collect()
        }
        "composer.json" => {
            #[derive(Deserialize)]
            struct ComposerJson {
                #[serde(default)]
                require: BTreeMap<String, serde_json::Value>,
                #[serde(default, rename = "require-dev")]
                require_dev: BTreeMap<String, serde_json::Value>,
            }
            serde_json::from_str::<ComposerJson>(text)
                .map(|composer| {
                    composer
                        .require
                        .into_keys()
                        .chain(composer.require_dev.into_keys())
                        // `php`, `ext-json` : plateforme, pas des paquets.
                        .filter(|name| name.contains('/'))
                        .map(|name| ("Packagist", name))
                        .collect()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

pub fn is_declaration(path: &str) -> bool {
    matches!(file_name(path), "pyproject.toml" | "composer.json")
}

/// Forme canonique d'un nom pour comparer manifeste et lockfile
/// (PEP 503 pour PyPI : casse et `-_.` indifférents).
pub fn normalized(ecosystem: &str, name: &str) -> String {
    if ecosystem == "PyPI" {
        let mut out = String::with_capacity(name.len());
        let mut last_dash = false;
        for c in name.chars() {
            if matches!(c, '-' | '_' | '.') {
                if !last_dash {
                    out.push('-');
                }
                last_dash = true;
            } else {
                out.push(c.to_ascii_lowercase());
                last_dash = false;
            }
        }
        out
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(found: &[Locked]) -> Vec<(String, String, Option<bool>)> {
        found
            .iter()
            .map(|l| (l.name.clone(), l.version.clone(), l.direct))
            .collect()
    }

    #[test]
    fn package_lock_v3_gives_installed_versions_and_direct_flag() {
        let lock = r#"{
          "lockfileVersion": 3,
          "packages": {
            "": { "dependencies": { "express": "^4.18.0" }, "devDependencies": { "@types/node": "^20" } },
            "node_modules/express": { "version": "4.21.1" },
            "node_modules/@types/node": { "version": "20.11.5", "dev": true },
            "node_modules/qs": { "version": "6.13.0" },
            "node_modules/express/node_modules/debug": { "version": "2.6.9" },
            "node_modules/my-alias": { "name": "lodash", "version": "4.17.21" },
            "node_modules/local": { "resolved": "packages/local", "link": true },
            "packages/local": { "version": "0.0.0" }
          }
        }"#;
        assert_eq!(
            versions(&npm_lock(lock)),
            vec![
                ("@types/node".into(), "20.11.5".into(), Some(true)),
                ("express".into(), "4.21.1".into(), Some(true)),
                ("debug".into(), "2.6.9".into(), Some(false)),
                ("lodash".into(), "4.17.21".into(), Some(false)),
                ("qs".into(), "6.13.0".into(), Some(false)),
            ]
        );
    }

    #[test]
    fn package_lock_v1_walks_nested_dependencies() {
        let lock = r#"{"lockfileVersion":1,"dependencies":{
            "a":{"version":"1.0.0","dependencies":{"b":{"version":"2.0.0"}}},
            "gitdep":{"version":"github:user/repo#abc"}}}"#;
        assert_eq!(
            versions(&npm_lock(lock)),
            vec![
                ("a".into(), "1.0.0".into(), None),
                ("b".into(), "2.0.0".into(), Some(false)),
            ]
        );
    }

    #[test]
    fn yarn_v1_and_berry() {
        let v1 = "# yarn lockfile v1\n\n\"@babel/core@^7.0.0\", \"@babel/core@^7.1.0\":\n  version \"7.23.2\"\n  resolved \"https://…\"\n\nlodash@^4.17.15:\n  version \"4.17.21\"\n";
        assert_eq!(
            versions(&yarn_lock(v1)),
            vec![
                ("@babel/core".into(), "7.23.2".into(), None),
                ("lodash".into(), "4.17.21".into(), None),
            ]
        );
        let berry = "__metadata:\n  version: 8\n\n\"lodash@npm:^4.17.15\":\n  version: 4.17.21\n\n\"app@workspace:.\":\n  version: 0.0.0-use.local\n";
        assert_eq!(
            versions(&yarn_lock(berry)),
            vec![("lodash".into(), "4.17.21".into(), None)]
        );
    }

    #[test]
    fn pnpm_v5_v6_and_v9_keys() {
        let lock = "lockfileVersion: '9.0'\n\nimporters:\n  .:\n    dependencies:\n      lodash:\n        version: 4.17.21\n\npackages:\n\n  lodash@4.17.21:\n    resolution: {integrity: sha512-x}\n\n  '@babel/core@7.23.2':\n    resolution: {integrity: sha512-y}\n\n  /react-dom@18.2.0(react@18.2.0):\n    resolution: {integrity: sha512-z}\n\n  /@scope/old/1.0.0_peer@2.0.0:\n    resolution: {integrity: sha512-w}\n\nsnapshots:\n\n  ignored@9.9.9:\n    dependencies: {}\n";
        assert_eq!(
            versions(&pnpm_lock(lock)),
            vec![
                ("lodash".into(), "4.17.21".into(), None),
                ("@babel/core".into(), "7.23.2".into(), None),
                ("react-dom".into(), "18.2.0".into(), None),
                ("@scope/old".into(), "1.0.0".into(), None),
            ]
        );
    }

    #[test]
    fn cargo_lock_skips_workspace_and_git_crates() {
        let lock = r#"
version = 3

[[package]]
name = "app"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "forked"
version = "0.3.0"
source = "git+https://github.com/x/forked#abc"
"#;
        assert_eq!(
            versions(&cargo_lock(lock)),
            vec![("serde".into(), "1.0.200".into(), None)]
        );
    }

    #[test]
    fn python_locks() {
        let poetry = "[[package]]\nname = \"requests\"\nversion = \"2.31.0\"\n\n[[package]]\nname = \"mine\"\nversion = \"0.1.0\"\n[package.source]\ntype = \"directory\"\nurl = \"../mine\"\n";
        assert_eq!(
            versions(&python_toml_lock(poetry)),
            vec![("requests".into(), "2.31.0".into(), None)]
        );
        let uv = "version = 1\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\nsource = { editable = \".\" }\n\n[[package]]\nname = \"jinja2\"\nversion = \"3.1.2\"\nsource = { registry = \"https://pypi.org/simple\" }\n";
        assert_eq!(
            versions(&python_toml_lock(uv)),
            vec![("jinja2".into(), "3.1.2".into(), None)]
        );
        let pipfile = r#"{"default":{"django":{"version":"==4.2.1"}},"develop":{"pytest":{"version":"==7.4.0"}}}"#;
        assert_eq!(
            versions(&pipfile_lock(pipfile)),
            vec![
                ("django".into(), "4.2.1".into(), None),
                ("pytest".into(), "7.4.0".into(), None),
            ]
        );
    }

    #[test]
    fn composer_and_go() {
        let composer = r#"{"packages":[{"name":"symfony/http-kernel","version":"v6.4.1"},{"name":"acme/dev","version":"dev-main"}],"packages-dev":[]}"#;
        assert_eq!(
            versions(&composer_lock(composer)),
            vec![("symfony/http-kernel".into(), "6.4.1".into(), None)]
        );
        let go = "module x\n\ngo 1.22\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgolang.org/x/net v0.17.0 // indirect\n)\n\nrequire github.com/google/uuid v1.6.0\n";
        assert_eq!(
            versions(&go_mod(go)),
            vec![
                (
                    "github.com/gin-gonic/gin".into(),
                    "1.9.1".into(),
                    Some(true)
                ),
                ("golang.org/x/net".into(), "0.17.0".into(), Some(false)),
                ("github.com/google/uuid".into(), "1.6.0".into(), Some(true)),
            ]
        );
    }

    #[test]
    fn declared_names_from_pyproject_and_composer() {
        let pyproject = "[project]\ndependencies = [\"Requests>=2\", \"jinja2[i18n]==3.1\"]\n\n[tool.poetry.dependencies]\npython = \"^3.11\"\nflask = \"^3\"\n";
        let names: Vec<String> = declared_names("pyproject.toml", pyproject)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(names, vec!["Requests", "jinja2", "flask"]);
        let composer = r#"{"require":{"php":">=8.1","ext-json":"*","symfony/console":"^6"}}"#;
        assert_eq!(
            declared_names("composer.json", composer),
            vec![("Packagist", "symfony/console".to_owned())]
        );
        assert_eq!(normalized("PyPI", "Jinja2__Ext.x"), "jinja2-ext-x");
    }
}
