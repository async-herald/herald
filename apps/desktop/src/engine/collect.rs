//! Collecte des fichiers d'un projet, avec les mêmes règles que le backend quand il
//! récupère un dépôt (`herald-app/src/source/filter.rs` et `source/mod.rs`) : mêmes
//! dossiers ignorés, même taille maximale, même bruit écarté, mêmes fichiers gardés.

use std::fs;
use std::path::Path;

use herald_engine::builtins::devops;
use herald_engine::config::{CONFIG_FILE, Config};
use herald_engine::dependencies::manifest;
use herald_engine::language::Language;
use herald_engine::types::FileInput;

/// Nombre maximal de fichiers analysés (`MAX_FILES` du backend).
pub const MAX_FILES: usize = 5_000;
/// Taille maximale d'un fichier (`MAX_FILE_BYTES` du backend).
pub const MAX_FILE_BYTES: u64 = 500 * 1024;

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    "dist",
    "build",
    "target",
    "bin",
    "obj",
    "out",
    ".git",
    ".claude",
    ".idea",
    ".vscode",
    "__pycache__",
    ".next",
    "coverage",
    ".cache",
    ".turbo",
    ".vercel",
    "generated",
    "storybook-static",
];

/// Fichiers retenus, chemins relatifs à `root` avec des `/`, dans un ordre stable.
pub fn collect_files(root: &Path) -> Vec<FileInput> {
    let mut candidates = Vec::new();
    walk(root, root, &mut candidates);
    candidates.sort();

    candidates
        .into_iter()
        .filter_map(|relative| {
            let code = fs::read_to_string(root.join(&relative)).ok()?;
            Some(FileInput {
                path: relative,
                code,
            })
        })
        .take(MAX_FILES)
        .collect()
}

/// Tous les fichiers du projet pour l'explorateur (dossiers ignorés écartés,
/// aucun filtre de taille ni de type), chemins relatifs avec des `/`, triés.
pub fn project_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_all(root, root, &mut out);
    out.sort();
    out.truncate(MAX_TREE_FILES);
    out
}

/// Limite de l'explorateur, pour garder l'arbre lisible et rapide.
const MAX_TREE_FILES: usize = 20_000;

fn walk_all(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_TREE_FILES {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            if !SKIP_DIRS.iter().any(|skip| name == *skip) && name != ".DS_Store" {
                walk_all(root, &path, out);
            }
        } else if kind.is_file() && entry.file_name() != ".DS_Store" {
            if let Ok(relative) = path.strip_prefix(root) {
                out.push(
                    relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // Les liens symboliques ne sont pas suivis (ni dossiers ni fichiers).
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if kind.is_dir() {
            let name = entry.file_name();
            if !SKIP_DIRS.iter().any(|skip| name == *skip) {
                walk(root, &path, out);
            }
        } else if kind.is_file() {
            let size = entry.metadata().map_or(u64::MAX, |m| m.len());
            if keep(&relative, size) {
                out.push(relative);
            }
        }
    }
}

/// Copie de `filter::keep` du backend.
fn keep(path: &str, size: u64) -> bool {
    if size > MAX_FILE_BYTES {
        return false;
    }
    if path.split('/').any(|seg| SKIP_DIRS.contains(&seg)) {
        return false;
    }
    // `.github` reste ignoré, sauf les workflows CI (matière de la catégorie DevOps).
    if path.split('/').any(|seg| seg == ".github") && !devops::is_ci_workflow(path) {
        return false;
    }
    if is_noise(path) {
        return false;
    }
    Language::from_path(path).is_some()
        || manifest::is_manifest(path)
        || is_sensitive(path)
        || is_web_asset(path)
        || devops::is_dockerfile(path)
        || devops::is_ci_workflow(path)
}

fn is_web_asset(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    [".html", ".htm", ".xhtml", ".css", ".scss", ".sass", ".less"]
        .iter()
        .any(|e| name.ends_with(e))
}

fn is_noise(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if name.ends_with(".min.js")
        || name.ends_with(".min.mjs")
        || name.ends_with(".min.css")
        || name.ends_with(".bundle.js")
        || name.ends_with(".map")
    {
        return true;
    }
    if name.ends_with(".d.ts") || name.contains(".generated.") || name.contains(".gen.") {
        return true;
    }
    if lower.split('/').any(|s| {
        matches!(
            s,
            "seeds" | "fixtures" | "snapshots" | "__snapshots__" | "__mocks__"
        )
    }) {
        return true;
    }
    name.starts_with("seed-") || name.starts_with("seed.")
}

fn is_sensitive(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name.starts_with(".env")
        || matches!(
            name.as_str(),
            "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
                | "credentials.json"
                | ".npmrc"
                | ".pypirc"
                | ".netrc"
                | ".htpasswd"
                | "kubeconfig"
                | ".dockercfg"
                | ".pgpass"
                | ".boto"
                | ".s3cfg"
                | "terraform.tfstate"
                | "terraform.tfstate.backup"
        )
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".ppk"]
            .iter()
            .any(|e| name.ends_with(e))
}

/// Applique le `herald.config.json` à la racine du projet, comme `apply_repo_config`
/// du backend : fusionne `ignore`, `rules.disable` et `thresholds`, puis retire les
/// fichiers ignorés. Le fichier de configuration n'est pas lui-même analysé (le
/// backend ne le garde pas non plus) : le nombre de fichiers, donc la note, reste le même.
pub fn apply_repo_config(root: &Path, config: &mut Config, files: &mut Vec<FileInput>) {
    let Ok(raw) = fs::read_to_string(root.join(CONFIG_FILE)) else {
        return;
    };
    let repo_cfg: Config = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("herald: {CONFIG_FILE} invalide ({e})");
            return;
        }
    };
    config.ignore = repo_cfg.ignore;
    config.rules = repo_cfg.rules;
    config.thresholds = repo_cfg.thresholds;
    if !config.ignore.is_empty() {
        let ignore = config.ignore.clone();
        files.retain(|f| !path_ignored(&f.path, &ignore));
    }
}

fn path_ignored(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        let pat = pat.trim_matches('/');
        !pat.is_empty() && path.split('/').any(|c| c == pat)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_code_and_drops_noise_like_the_backend() {
        assert!(keep("src/main.rs", 100));
        assert!(keep("package.json", 100));
        assert!(keep(".env", 10));
        assert!(keep("index.html", 10));
        assert!(keep(".github/workflows/ci.yml", 10));
        assert!(!keep(".github/ISSUE_TEMPLATE/bug.md", 10));
        assert!(!keep("node_modules/pkg/index.js", 10));
        assert!(!keep("dist/app.min.js", 10));
        assert!(!keep("src/types.d.ts", 10));
        assert!(!keep("README.md", 10));
        assert!(!keep("src/huge.rs", MAX_FILE_BYTES + 1));
    }

    #[test]
    fn repo_config_filters_ignored_paths() {
        let root = std::env::temp_dir().join(format!("herald-config-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(CONFIG_FILE), r#"{"ignore":["styles"]}"#).unwrap();
        let mut config = Config::default();
        let mut files = vec![
            FileInput {
                path: "app/styles/theme.css".into(),
                code: String::new(),
            },
            FileInput {
                path: "app/page.tsx".into(),
                code: String::new(),
            },
        ];
        apply_repo_config(&root, &mut config, &mut files);
        fs::remove_dir_all(&root).ok();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "app/page.tsx");
    }
}
