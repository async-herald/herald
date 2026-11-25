//! Collecte des fichiers d'un projet, avec les mêmes règles que le backend quand il
//! récupère un dépôt (`herald-app/src/source/filter.rs` et `source/mod.rs`) : mêmes
//! dossiers ignorés, même taille maximale, même bruit écarté, mêmes fichiers gardés.
//!
//! En local, le dossier contient aussi ce qu'un clone n'aurait pas : fichiers
//! ignorés par Git, caches, environnements et produits de compilation. Le
//! parcours les écarte (`walk_project`) : règles `.gitignore` / `.ignore` /
//! `.git/info/exclude` à tous les niveaux, dossiers cachés (sauf ceux de
//! configuration), dossiers générés connus, et dossiers marqués comme cache
//! (`CACHEDIR.TAG`), environnement Python (`pyvenv.cfg`) ou paquet compilé
//! (`.app`, `.framework`…).

use std::fs;
use std::path::Path;

use ignore::{DirEntry, WalkBuilder};

use herald_engine::builtins::devops;
use herald_engine::config::{CONFIG_FILE, Config};
use herald_engine::dependencies::manifest;
use herald_engine::language::Language;
use herald_engine::types::FileInput;

/// Limites proposées dans Réglages > Analyse (le backend gardait 5 000 fichiers ;
/// l'analyse étant locale, la limite suit les gros dépôts).
pub const FILE_LIMITS: [usize; 4] = [5_000, 20_000, 50_000, 100_000];
/// Limite par défaut, tant que l'utilisateur n'en a pas choisi une autre.
pub const DEFAULT_MAX_FILES: usize = 50_000;
/// Réglage local : nombre maximal de fichiers analysés.
pub const MAX_FILES_SETTING: &str = "max_files";
/// Code lu au plus pour une analyse. Le moteur garde tous les fichiers en
/// mémoire en même temps (ses règles croisent les fichiers) : ce budget borne
/// la mémoire, quelle que soit la limite de fichiers.
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

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

/// Dossiers générés par les outils (caches, dépendances installées, sorties de
/// compilation), en plus de `SKIP_DIRS` ; les dossiers cachés sont traités à part.
const GENERATED_DIRS: &[&str] = &[
    "DerivedData",
    "Pods",
    "Carthage",
    "xcuserdata",
    "venv",
    "bower_components",
    "jspm_packages",
    "site-packages",
    "htmlcov",
    "_build",
    "deps",
    "elm-stuff",
    "zig-cache",
    "zig-out",
];

/// Suffixes de dossiers compilés ou d'empaquetage (`App.app`, `Capacitor.framework`…).
const GENERATED_DIR_SUFFIXES: &[&str] = &[
    ".app",
    ".framework",
    ".xcframework",
    ".dsym",
    ".xcarchive",
    ".egg-info",
];

/// Dossiers cachés qui contiennent de la configuration à analyser (CI, hooks,
/// conteneurs de développement) ; les autres (`.venv`, `.gradle`, `.utmp`…)
/// sont des caches ou des réglages d'outils.
const KEPT_HIDDEN_DIRS: &[&str] = &[
    ".github",
    ".gitlab",
    ".circleci",
    ".buildkite",
    ".gitea",
    ".forgejo",
    ".woodpecker",
    ".tekton",
    ".devcontainer",
    ".docker",
    ".husky",
    ".config",
    ".storybook",
    ".well-known",
    ".changeset",
    ".cargo",
];

/// Fichiers qui marquent un dossier comme cache (norme `CACHEDIR.TAG`) ou
/// comme environnement virtuel Python.
const GENERATED_DIR_MARKERS: &[&str] = &["CACHEDIR.TAG", "pyvenv.cfg"];

/// Dossier à ne pas parcourir (voir l'en-tête du module).
fn is_skipped_dir(name: &str, path: &Path) -> bool {
    if SKIP_DIRS.contains(&name) || GENERATED_DIRS.contains(&name) || name == ".DS_Store" {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    if GENERATED_DIR_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        return true;
    }
    if name.starts_with('.') && !KEPT_HIDDEN_DIRS.contains(&name) {
        return true;
    }
    GENERATED_DIR_MARKERS
        .iter()
        .any(|marker| path.join(marker).is_file())
}

/// Parcourt les fichiers du projet (liens symboliques non suivis) et appelle
/// `visit` avec le chemin relatif (séparateur `/`) et la taille ; `visit`
/// renvoie `false` pour arrêter.
fn walk_project(root: &Path, mut visit: impl FnMut(String, u64) -> bool) {
    let walker = WalkBuilder::new(root)
        // Les dossiers cachés sont filtrés par `is_skipped_dir` ; les fichiers
        // cachés (`.env`, `.gitlab-ci.yml`) restent analysés.
        .hidden(false)
        // Seulement les règles du projet : pas celles des dossiers parents ni
        // l'exclusion globale de l'utilisateur, pour un résultat reproductible.
        .parents(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry: &DirEntry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !is_skipped_dir(&entry.file_name().to_string_lossy(), entry.path())
        })
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let relative = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let size = entry.metadata().map_or(u64::MAX, |m| m.len());
        if !visit(relative, size) {
            return;
        }
    }
}

/// Fichiers lus pour l'analyse.
pub struct Collected {
    pub files: Vec<FileInput>,
    /// Fichiers qui auraient été analysés sans limite (`files.len()` au moins).
    pub candidates: usize,
}

/// Fichiers retenus, chemins relatifs à `root` avec des `/`, dans un ordre
/// stable. Au plus `max_files` fichiers et [`MAX_TOTAL_BYTES`] de code, pris à
/// tour de rôle dans chaque dossier de premier niveau quand tout ne tient pas.
/// `on_read(lus, à lire)` suit la lecture.
pub fn collect_files(
    root: &Path,
    max_files: usize,
    mut on_read: impl FnMut(usize, usize),
) -> Collected {
    let mut candidates = Vec::new();
    walk_project(root, |relative, size| {
        if keep(&relative, size) {
            candidates.push((relative, size));
        }
        true
    });
    let total = candidates.len();
    let chosen = select_fairly(candidates, max_files, MAX_TOTAL_BYTES);

    let to_read = chosen.len();
    let mut files = Vec::with_capacity(to_read);
    for (ix, relative) in chosen.into_iter().enumerate() {
        if let Ok(code) = fs::read_to_string(root.join(&relative)) {
            files.push(FileInput {
                path: relative,
                code,
            });
        }
        if ix % 250 == 0 || ix + 1 == to_read {
            on_read(ix + 1, to_read);
        }
    }
    Collected {
        files,
        candidates: total,
    }
}

/// Choisit au plus `max_files` fichiers et `max_bytes` octets parmi
/// `candidates` (chemin, taille). Tout tient : tout est gardé. Sinon les
/// dossiers de premier niveau (les paquets d'un monorepo, `src`, `tests`…)
/// fournissent un fichier chacun à leur tour, pour qu'aucun ne soit écarté en
/// bloc par l'ordre alphabétique. Résultat trié.
fn select_fairly(
    mut candidates: Vec<(String, u64)>,
    max_files: usize,
    max_bytes: u64,
) -> Vec<String> {
    candidates.sort();
    let bytes: u64 = candidates.iter().map(|(_, size)| *size).sum();
    if candidates.len() <= max_files && bytes <= max_bytes {
        return candidates.into_iter().map(|(path, _)| path).collect();
    }

    // Files par dossier de premier niveau, dans l'ordre des chemins.
    let mut groups: Vec<(String, std::collections::VecDeque<(String, u64)>)> = Vec::new();
    for (path, size) in candidates {
        let top = path.split_once('/').map_or("", |(top, _)| top).to_owned();
        match groups.last_mut() {
            Some((name, queue)) if *name == top => queue.push_back((path, size)),
            _ => groups.push((top, std::collections::VecDeque::from([(path, size)]))),
        }
    }

    let mut chosen = Vec::new();
    let mut used = 0u64;
    while chosen.len() < max_files && groups.iter().any(|(_, queue)| !queue.is_empty()) {
        for (_, queue) in &mut groups {
            if chosen.len() >= max_files {
                break;
            }
            // Un fichier qui dépasse le budget restant est passé ; un plus petit
            // du même dossier peut encore tenir.
            while let Some((path, size)) = queue.pop_front() {
                if used + size <= max_bytes {
                    used += size;
                    chosen.push(path);
                    break;
                }
            }
        }
    }
    chosen.sort();
    chosen
}

/// Chemins des fichiers que l'analyse retiendrait, sans les lire (carte du
/// code de l'onglet Général), triés.
pub fn analyzed_paths(root: &Path, max_files: usize) -> Vec<String> {
    let mut candidates = Vec::new();
    walk_project(root, |relative, size| {
        if keep(&relative, size) {
            candidates.push((relative, size));
        }
        true
    });
    select_fairly(candidates, max_files, MAX_TOTAL_BYTES)
}

/// Tous les fichiers du projet pour l'explorateur (dossiers ignorés ou générés
/// écartés, aucun filtre de taille ni de type), chemins relatifs avec des `/`, triés.
pub fn project_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_project(root, |relative, _| {
        if !relative.ends_with(".DS_Store") {
            out.push(relative);
        }
        out.len() < MAX_TREE_FILES
    });
    out.sort();
    out
}

/// Limite de l'explorateur, pour garder l'arbre lisible et rapide.
const MAX_TREE_FILES: usize = 100_000;

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

    fn files(paths: &[&str]) -> Vec<(String, u64)> {
        paths.iter().map(|path| (path.to_string(), 10)).collect()
    }

    #[test]
    fn everything_is_kept_when_it_fits() {
        let chosen = select_fairly(files(&["b.rs", "a/x.rs", "a/y.rs"]), 10, 1_000);
        assert_eq!(chosen, vec!["a/x.rs", "a/y.rs", "b.rs"]);
    }

    #[test]
    fn over_the_limit_every_top_folder_gets_its_turn() {
        // Par ordre alphabétique, `apps/` aurait tout pris.
        let candidates = files(&[
            "apps/a1.ts",
            "apps/a2.ts",
            "apps/a3.ts",
            "apps/a4.ts",
            "packages/p1.ts",
            "packages/p2.ts",
            "services/s1.go",
        ]);
        let chosen = select_fairly(candidates, 4, u64::MAX);
        assert_eq!(
            chosen,
            vec![
                "apps/a1.ts",
                "apps/a2.ts",
                "packages/p1.ts",
                "services/s1.go"
            ]
        );
    }

    #[test]
    fn the_byte_budget_bounds_memory() {
        let candidates = vec![
            ("a/big.ts".to_string(), 900),
            ("a/small.ts".to_string(), 50),
            ("b/small.ts".to_string(), 50),
        ];
        // 960 octets : le gros fichier et un petit tiennent, pas le troisième.
        let chosen = select_fairly(candidates, 10, 960);
        assert_eq!(chosen, vec!["a/big.ts", "b/small.ts"]);
    }

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
    fn local_walk_skips_ignored_and_generated_folders() {
        let root = std::env::temp_dir().join(format!("herald-walk-test-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        let files = [
            "src/app.js",
            ".env",
            ".gitlab-ci.yml",
            ".github/workflows/ci.yml",
            "ios/.gitignore",
            "ios/App/AppDelegate.swift",
            // Ignoré par ios/.gitignore.
            "ios/DerivedData/Build/x.js",
            "ios/logs/run.js",
            // Dossier caché d'outil.
            ".utmp/session.js",
            // Environnement Python et cache marqués.
            "env/pyvenv.cfg",
            "env/lib/site.py",
            "cache/CACHEDIR.TAG",
            "cache/a.js",
            // Paquet compilé.
            "ios/Capacitor.framework/Headers/x.js",
            "Pods/Lib/y.swift",
        ];
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x").unwrap();
        }
        fs::write(root.join("ios/.gitignore"), "DerivedData\nlogs/\n").unwrap();

        let listed = project_files(&root);
        let analyzed: Vec<String> = collect_files(&root, DEFAULT_MAX_FILES, |_, _| {})
            .files
            .into_iter()
            .map(|f| f.path)
            .collect();
        fs::remove_dir_all(&root).ok();

        assert_eq!(
            listed,
            [
                ".env",
                ".github/workflows/ci.yml",
                ".gitlab-ci.yml",
                "ios/.gitignore",
                "ios/App/AppDelegate.swift",
                "src/app.js",
            ]
        );
        assert!(analyzed.contains(&"src/app.js".to_owned()));
        assert!(analyzed.contains(&".github/workflows/ci.yml".to_owned()));
        assert!(!analyzed.iter().any(|path| path.contains("DerivedData")));
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
