mod extract;
mod graph;
mod lang;

use std::collections::{HashMap, HashSet};

use crate::category::Category;
use crate::config::Config;
use crate::language::Language;
use tree_sitter::Tree;

use crate::types::{FileInput, Issue, Severity};

use extract::FileSymbols;

struct Node {
    path: String,
    lang: Language,
    sym: FileSymbols,
}

pub fn analyze(
    files: &[FileInput],
    parsed: &[(Option<Language>, Option<Tree>)],
    config: &Config,
) -> HashMap<String, Vec<Issue>> {
    let mut out: HashMap<String, Vec<Issue>> = HashMap::new();

    // Règles « git » par chemin de fichier (sur la liste brute, indépendamment du
    // langage : .env / clés n'ont pas de langage reconnu).
    detect_committed_files(files, config, &mut out);
    detect_repo_hygiene(files, config, &mut out);

    let nodes: Vec<Node> = files
        .iter()
        .zip(parsed)
        .filter_map(|(f, (lang, tree))| {
            let lang = (*lang)?;
            Some(Node {
                path: f.path.clone(),
                lang,
                sym: extract::extract(tree.as_ref(), &f.code, lang),
            })
        })
        .collect();

    if nodes.is_empty() {
        return out;
    }

    let path_to_idx: HashMap<&str, usize> =
        nodes.iter().enumerate().map(|(i, n)| (n.path.as_str(), i)).collect();
    let file_set: HashSet<&str> = nodes.iter().map(|n| n.path.as_str()).collect();

    let mut adj = vec![Vec::new(); nodes.len()];
    let mut indeg = vec![0usize; nodes.len()];
    let mut star_used: HashSet<usize> = HashSet::new();

    for (i, n) in nodes.iter().enumerate() {
        for (spec, _) in &n.sym.import_specs {
            if let Some(j) = resolve_idx(n, spec, &file_set, &path_to_idx)
                && j != i {
                    adj[i].push(j);
                    indeg[j] += 1;
                }
        }
        for spec in &n.sym.star_specs {
            if let Some(j) = resolve_idx(n, spec, &file_set, &path_to_idx) {
                star_used.insert(j);
            }
        }
    }

    if config.is_rule_enabled("import-cycle") {
        detect_cycles(&nodes, &adj, &file_set, &path_to_idx, &mut out);
    }
    if config.is_rule_enabled("orphan-module") {
        detect_orphans(&nodes, &indeg, &mut out);
    }
    if config.is_rule_enabled("unused-export") {
        detect_unused_exports(&nodes, &star_used, &mut out);
    }

    out
}

fn resolve_idx(
    n: &Node,
    spec: &str,
    file_set: &HashSet<&str>,
    path_to_idx: &HashMap<&str, usize>,
) -> Option<usize> {
    let target = lang::resolve(&n.path, n.lang, spec, file_set)?;
    path_to_idx.get(target.as_str()).copied()
}

fn detect_cycles(
    nodes: &[Node],
    adj: &[Vec<usize>],
    file_set: &HashSet<&str>,
    path_to_idx: &HashMap<&str, usize>,
    out: &mut HashMap<String, Vec<Issue>>,
) {
    for scc in graph::sccs(adj) {
        if scc.len() < 2 {
            continue;
        }
        // Un seul relevé par cycle plutôt qu'un par fichier membre — sinon un
        // cycle de N fichiers (fréquent en Python : `__init__.py` qui
        // ré-exporte des sous-modules qui s'importent entre eux, un pattern
        // délibéré et sans bug) produit N warnings pour un seul problème réel
        // (ex: 20 warnings sur Flask pour un unique cycle). Le fichier
        // représentatif est le premier par ordre alphabétique de chemin —
        // déterministe, indépendant de l'ordre de traversée du dépôt.
        let members: HashSet<usize> = scc.iter().copied().collect();
        let v = *scc.iter().min_by_key(|&&i| nodes[i].path.as_str()).expect("scc non vide");

        let line = edge_line_into(&nodes[v], &members, file_set, path_to_idx);
        let others: Vec<&str> = scc
            .iter()
            .filter(|&&u| u != v)
            .map(|&u| filename(&nodes[u].path))
            .collect();
        push(out, &nodes[v].path, Issue {
            rule: "import-cycle".into(),
            category: Category::Pattern,
            severity: Severity::Warning,
            message: format!("Cycle d'imports ({} fichiers : {})", scc.len(), others.join(", ")),
            line,
            column: 1,
        });
    }
}

fn detect_orphans(nodes: &[Node], indeg: &[usize], out: &mut HashMap<String, Vec<Issue>>) {
    for (i, n) in nodes.iter().enumerate() {
        if !supports_exports(n.lang) || indeg[i] != 0 || n.sym.exports.is_empty() || is_entry(&n.path)
        {
            continue;
        }
        let line = n.sym.exports.first().map(|(_, l)| *l).unwrap_or(1);
        push(out, &n.path, Issue {
            rule: "orphan-module".into(),
            category: Category::Quality,
            severity: Severity::Info,
            message: "Module jamais importé (code potentiellement mort)".into(),
            line,
            column: 1,
        });
    }
}

fn detect_unused_exports(
    nodes: &[Node],
    star_used: &HashSet<usize>,
    out: &mut HashMap<String, Vec<Issue>>,
) {
    let mut used: HashSet<&str> = HashSet::new();
    for n in nodes {
        for name in &n.sym.named_imports {
            used.insert(name.as_str());
        }
    }

    for (i, n) in nodes.iter().enumerate() {
        if !supports_exports(n.lang) || n.sym.is_barrel || star_used.contains(&i) {
            continue;
        }
        for (name, line) in &n.sym.exports {
            if name == "default" || used.contains(name.as_str()) {
                continue;
            }
            push(out, &n.path, Issue {
                rule: "unused-export".into(),
                category: Category::Quality,
                severity: Severity::Info,
                message: format!("Export `{name}` jamais importé ailleurs"),
                line: *line,
                column: 1,
            });
        }
    }
}

fn edge_line_into(
    n: &Node,
    members: &HashSet<usize>,
    file_set: &HashSet<&str>,
    path_to_idx: &HashMap<&str, usize>,
) -> usize {
    for (spec, line) in &n.sym.import_specs {
        if let Some(j) = resolve_idx(n, spec, file_set, path_to_idx)
            && members.contains(&j) {
                return *line;
            }
    }
    1
}

fn push(out: &mut HashMap<String, Vec<Issue>>, path: &str, issue: Issue) {
    out.entry(path.to_string()).or_default().push(issue);
}

/// Noms de fichiers de secrets/clés qui ne devraient jamais être versionnés.
const SECRET_FILENAMES: &[&str] = &[
    "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", "credentials.json", ".npmrc", ".pypirc",
    ".netrc", ".htpasswd", "kubeconfig", ".dockercfg", ".pgpass", ".boto", ".s3cfg",
    "terraform.tfstate", "terraform.tfstate.backup",
];
const SECRET_EXTS: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".ppk"];

/// Règles « git » détectables sur le snapshot : fichiers sensibles versionnés.
/// Repo hygiene that needs a whole-repo view (one finding per dir/repo, not per
/// file — avoids spamming on dirs like dist/). Files committed against
/// `.gitignore` are already filtered out by the collector.
fn detect_repo_hygiene(
    files: &[FileInput],
    config: &Config,
    out: &mut HashMap<String, Vec<Issue>>,
) {
    // Several lockfiles from different package managers in the same repo.
    if config.is_rule_enabled("multiple-lockfiles") {
        let mut kinds = std::collections::BTreeSet::new();
        let mut anchor: Option<&str> = None;
        for f in files {
            let n = filename(&f.path);
            if matches!(n, "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "npm-shrinkwrap.json") {
                kinds.insert(n);
                anchor.get_or_insert(f.path.as_str());
            }
        }
        if kinds.len() >= 2 {
            if let Some(path) = anchor {
                push(out, path, Issue {
                    rule: "multiple-lockfiles".into(),
                    category: Category::Git,
                    severity: Severity::Warning,
                    message: format!(
                        "Plusieurs lockfiles versionnés ({}) — garde un seul gestionnaire de paquets",
                        kinds.iter().copied().collect::<Vec<_>>().join(", ")
                    ),
                    line: 1,
                    column: 1,
                });
            }
        }
    }

    // Build/output directories under version control (one finding per dir root).
    if config.is_rule_enabled("build-output-committed") {
        const DIRS: &[&str] = &["node_modules", "dist", "build", "target", ".next", "out", "__pycache__"];
        // dir -> a real file under it, used to anchor the single finding.
        let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        for f in files {
            for d in DIRS {
                if f.path.starts_with(&format!("{d}/")) || f.path.contains(&format!("/{d}/")) {
                    seen.entry(d).or_insert(f.path.as_str());
                }
            }
        }
        for (d, anchor) in seen {
            push(out, anchor, Issue {
                rule: "build-output-committed".into(),
                category: Category::Git,
                severity: Severity::Warning,
                message: format!("Dossier de build versionné ({d}/) — ajoute-le au .gitignore"),
                line: 1,
                column: 1,
            });
        }
    }
}

fn detect_committed_files(
    files: &[FileInput],
    config: &Config,
    out: &mut HashMap<String, Vec<Issue>>,
) {
    let env_on = config.is_rule_enabled("committed-env-file");
    let secret_on = config.is_rule_enabled("committed-secret-file");
    let pk_on = config.is_rule_enabled("private-key-block");
    if !env_on && !secret_on && !pk_on {
        return;
    }
    for f in files {
        let name = filename(&f.path).to_ascii_lowercase();

        // Bloc de clé privée en clair dans le contenu (n'importe quel fichier versionné).
        if pk_on {
            if let Some((idx, _)) = f
                .code
                .lines()
                .enumerate()
                .find(|(_, l)| l.contains("BEGIN") && l.contains("PRIVATE KEY"))
            {
                push(
                    out,
                    &f.path,
                    Issue {
                        rule: "private-key-block".into(),
                        category: Category::Security,
                        severity: Severity::Error,
                        message: "Clé privée en clair dans un fichier versionné — à retirer et révoquer".into(),
                        line: idx + 1,
                        column: 1,
                    },
                );
            }
        }

        // .env versionné (hors templates .env.example / .env.sample / .env.template).
        if env_on
            && name.starts_with(".env")
            && !["example", "sample", "template", "dist", "schema"]
                .iter()
                .any(|s| name.contains(s))
        {
            push(
                out,
                &f.path,
                Issue {
                    rule: "committed-env-file".into(),
                    category: Category::Security,
                    severity: Severity::Error,
                    message: "Fichier .env versionné — secrets et configuration potentiellement exposés".into(),
                    line: 1,
                    column: 1,
                },
            );
            continue;
        }

        // Clés privées / fichiers de credentials versionnés.
        if secret_on
            && (SECRET_FILENAMES.contains(&name.as_str())
                || SECRET_EXTS.iter().any(|e| name.ends_with(e))
                || (name.starts_with("serviceaccount") && name.ends_with(".json")))
        {
            push(
                out,
                &f.path,
                Issue {
                    rule: "committed-secret-file".into(),
                    category: Category::Security,
                    severity: Severity::Warning,
                    message: format!(
                        "Fichier de secrets/clé versionné ({}) — à retirer du dépôt et révoquer",
                        filename(&f.path)
                    ),
                    line: 1,
                    column: 1,
                },
            );
        }
    }
}

fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn supports_exports(lang: Language) -> bool {
    matches!(lang, Language::JavaScript | Language::TypeScript)
}

fn is_entry(path: &str) -> bool {
    let file = filename(path);
    let stem = file.split('.').next().unwrap_or(file);
    const ENTRIES: &[&str] =
        &["index", "main", "app", "server", "cli", "setup", "__init__", "__main__"];
    ENTRIES.contains(&stem)
        || file.contains(".test.")
        || file.contains(".spec.")
        || path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
}
