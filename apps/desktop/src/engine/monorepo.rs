//! Monorepo : repère les paquets d'un dépôt pour noter chacun à part.
//!
//! D'abord ce que le dépôt déclare : `workspaces` de `package.json` (npm,
//! yarn), `pnpm-workspace.yaml`, `lerna.json`, `[workspace] members` de
//! `Cargo.toml`, `go.work`. Sans déclaration, les dossiers `apps/*`,
//! `packages/*`, `services/*`… qui ont leur propre manifeste. Moins de deux
//! paquets : ce n'est pas un monorepo.

use std::collections::BTreeSet;

use herald_engine::types::FileInput;

/// Fichiers qui font d'un dossier un paquet.
const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "composer.json",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "pubspec.yaml",
    "mix.exs",
];

/// Dossiers qui rangent habituellement les paquets d'un monorepo.
const CONVENTIONAL_PARENTS: &[&str] = &[
    "apps", "packages", "services", "libs", "modules", "crates", "projects", "plugins",
];

/// Un paquet du monorepo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Nom déclaré (`name` de package.json ou Cargo.toml), sinon le chemin.
    pub name: String,
    /// Dossier, relatif à la racine, sans `/` final.
    pub path: String,
}

/// Fichiers de la racine qui déclarent les paquets d'un workspace.
pub const WORKSPACE_FILES: &[&str] = &[
    "package.json",
    "pnpm-workspace.yaml",
    "lerna.json",
    "Cargo.toml",
    "go.work",
];

/// Paquets du dépôt, par chemin ; vide si ce n'est pas un monorepo.
/// `files` : fichiers lus pour l'analyse (manifestes compris, avec leur code) ;
/// `extra` : fichiers de workspace de la racine que l'analyse ne lit pas.
pub fn detect(files: &[FileInput], extra: &[FileInput]) -> Vec<Package> {
    let code = |path: &str| {
        files
            .iter()
            .chain(extra)
            .find(|f| f.path == path)
            .map(|f| f.code.as_str())
    };
    // Dossiers (hors racine) qui ont un manifeste.
    let candidates: BTreeSet<&str> = files
        .iter()
        .filter_map(|f| {
            let (dir, name) = f.path.rsplit_once('/')?;
            MANIFESTS.contains(&name).then_some(dir)
        })
        .collect();

    let patterns = declared_patterns(&code);
    let chosen: Vec<&str> = if patterns.is_empty() {
        candidates
            .iter()
            .copied()
            .filter(|dir| {
                dir.split_once('/').is_some_and(|(parent, rest)| {
                    CONVENTIONAL_PARENTS.contains(&parent) && !rest.contains('/')
                })
            })
            .collect()
    } else {
        candidates
            .iter()
            .copied()
            .filter(|dir| patterns.iter().any(|pattern| matches(pattern, dir)))
            .collect()
    };
    if chosen.len() < 2 {
        return Vec::new();
    }
    chosen
        .into_iter()
        .map(|dir| Package {
            name: declared_name(dir, &code).unwrap_or_else(|| dir.to_owned()),
            path: dir.to_owned(),
        })
        .collect()
}

/// Paquet le plus profond qui contient `path` ; `None` : hors de tout paquet.
pub fn package_of<'a>(packages: &'a [Package], path: &str) -> Option<&'a Package> {
    packages
        .iter()
        .filter(|p| {
            path.strip_prefix(&p.path)
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|p| p.path.len())
}

/// Motifs de paquets déclarés à la racine du dépôt.
fn declared_patterns<'a>(code: &impl Fn(&str) -> Option<&'a str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(json) =
        code("package.json").and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        let list = json["workspaces"]
            .as_array()
            .or_else(|| json["workspaces"]["packages"].as_array());
        out.extend(
            list.into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned)),
        );
    }
    if let Some(json) =
        code("lerna.json").and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        out.extend(
            json["packages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned)),
        );
    }
    if let Some(yaml) = code("pnpm-workspace.yaml") {
        // Liste `packages:` : lignes « - 'motif' ».
        out.extend(
            yaml.lines()
                .filter_map(|line| line.trim().strip_prefix('-'))
                .map(|item| {
                    item.trim()
                        .trim_matches(|c| c == '\'' || c == '"')
                        .to_owned()
                }),
        );
    }
    if let Some(toml) = code("Cargo.toml") {
        out.extend(cargo_members(toml));
    }
    if let Some(go_work) = code("go.work") {
        out.extend(go_work_uses(go_work));
    }
    out.into_iter()
        .filter(|p| !p.is_empty() && !p.starts_with('!'))
        .map(|p| p.trim_start_matches("./").trim_end_matches('/').to_owned())
        .collect()
}

/// `members = ["crates/*", "app"]` de la section `[workspace]`.
fn cargo_members(toml: &str) -> Vec<String> {
    let Some(start) = toml.find("[workspace]") else {
        return Vec::new();
    };
    let section = &toml[start..];
    let Some(members) = section.find("members") else {
        return Vec::new();
    };
    let after = &section[members..];
    let (Some(open), Some(close)) = (after.find('['), after.find(']')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    after[open + 1..close]
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// `use ./a` ou `use ( ./a ./b )` de go.work.
fn go_work_uses(go_work: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in go_work.lines().map(str::trim) {
        if in_block {
            if line.starts_with(')') {
                in_block = false;
            } else if !line.is_empty() && !line.starts_with("//") {
                out.push(line.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix("use") {
            let rest = rest.trim();
            if rest.starts_with('(') {
                in_block = true;
            } else if !rest.is_empty() {
                out.push(rest.to_owned());
            }
        }
    }
    out
}

/// Motif de workspace : chemin exact, `dossier/*` (un niveau) ou `dossier/**`.
fn matches(pattern: &str, dir: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return dir
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'));
    }
    let (pattern_parts, dir_parts): (Vec<&str>, Vec<&str>) =
        (pattern.split('/').collect(), dir.split('/').collect());
    pattern_parts.len() == dir_parts.len()
        && pattern_parts
            .iter()
            .zip(&dir_parts)
            .all(|(p, d)| *p == "*" || p == d)
}

/// `name` de package.json, ou de la section `[package]` de Cargo.toml.
fn declared_name<'a>(dir: &str, code: &impl Fn(&str) -> Option<&'a str>) -> Option<String> {
    if let Some(json) = code(&format!("{dir}/package.json"))
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
    {
        if let Some(name) = json["name"].as_str().filter(|n| !n.is_empty()) {
            return Some(name.to_owned());
        }
    }
    let toml = code(&format!("{dir}/Cargo.toml"))?;
    let package = &toml[toml.find("[package]")?..];
    package
        .lines()
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "name").then(|| value.trim().trim_matches('"').to_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::{Package, detect, package_of};
    use herald_engine::types::FileInput;

    fn file(path: &str, code: &str) -> FileInput {
        FileInput {
            path: path.into(),
            code: code.into(),
        }
    }

    #[test]
    fn npm_workspaces_name_each_package() {
        let files = vec![
            file(
                "package.json",
                r#"{"workspaces": ["apps/*", "packages/*"]}"#,
            ),
            file("apps/web/package.json", r#"{"name": "@acme/web"}"#),
            file("apps/web/src/index.ts", ""),
            file("packages/ui/package.json", r#"{"name": "@acme/ui"}"#),
            file("tools/script/package.json", r#"{"name": "script"}"#),
        ];
        assert_eq!(
            detect(&files, &[]),
            vec![
                Package {
                    name: "@acme/web".into(),
                    path: "apps/web".into()
                },
                Package {
                    name: "@acme/ui".into(),
                    path: "packages/ui".into()
                },
            ]
        );
    }

    #[test]
    fn pnpm_cargo_and_go_work_are_read() {
        let pnpm = vec![
            file("pnpm-workspace.yaml", "packages:\n  - 'libs/**'\n"),
            file("libs/a/package.json", "{}"),
            file("libs/b/c/package.json", "{}"),
        ];
        assert_eq!(detect(&pnpm, &[]).len(), 2);

        let cargo = vec![
            file(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/engine\", \"app\"]\n",
            ),
            file("crates/engine/Cargo.toml", "[package]\nname = \"engine\"\n"),
            file("app/Cargo.toml", "[package]\nname = \"app\"\n"),
        ];
        let names: Vec<String> = detect(&cargo, &[]).into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["app", "engine"]);

        let go = vec![
            file("go.work", "go 1.22\nuse (\n  ./api\n  ./worker\n)\n"),
            file("api/go.mod", "module api"),
            file("worker/go.mod", "module worker"),
        ];
        assert_eq!(
            detect(&[], &go).len(),
            0,
            "sans manifestes analysés, pas de paquet"
        );
        let (decl, manifests) = go.split_at(1);
        assert_eq!(detect(manifests, decl).len(), 2, "go.work lu à part");
    }

    #[test]
    fn conventional_folders_without_declaration() {
        let files = vec![
            file("services/auth/go.mod", ""),
            file("services/billing/pyproject.toml", ""),
            file("src/main.rs", ""),
        ];
        assert_eq!(detect(&files, &[]).len(), 2);
    }

    #[test]
    fn a_single_project_is_not_a_monorepo() {
        let files = vec![file("package.json", "{}"), file("src/index.ts", "")];
        assert!(detect(&files, &[]).is_empty());
    }

    #[test]
    fn a_file_belongs_to_its_deepest_package() {
        let packages = vec![
            Package {
                name: "a".into(),
                path: "apps".into(),
            },
            Package {
                name: "web".into(),
                path: "apps/web".into(),
            },
        ];
        assert_eq!(
            package_of(&packages, "apps/web/src/x.ts").unwrap().name,
            "web"
        );
        assert_eq!(package_of(&packages, "apps/readme.md").unwrap().name, "a");
        assert!(package_of(&packages, "appsx/y.ts").is_none());
        assert!(package_of(&packages, "README.md").is_none());
    }
}
