//! Mesure de précision des règles de sécurité sur un corpus de dépôts.
//!
//! ```text
//! cargo run --release --example precision -- <corpus> <sortie.jsonl>
//! ```
//!
//! `<corpus>` contient un dossier par dépôt (clone `--depth 1`). Chaque dépôt
//! est analysé comme dans l'app : même collecte de fichiers, catalogue du
//! moteur avec les règles de `rules/` à la place des siennes. Les manifestes
//! sont écartés (pas de requête réseau vers les registres). Une ligne JSON par
//! relevé de sécurité : dépôt, commit, fichier, ligne, règle et extrait (±2
//! lignes), pour l'étiquetage vrai / faux positif.

#[path = "../src/engine/collect.rs"]
#[allow(dead_code)]
mod collect;
#[path = "../src/engine/local_rules.rs"]
#[allow(dead_code)]
mod local_rules;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use herald_engine::category::Category;
use herald_engine::config::Config;
use herald_engine::dependencies::manifest;
use herald_engine::types::AnalyzeRequest;

const CONTEXT_LINES: usize = 2;
const MAX_LINE_CHARS: usize = 180;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (corpus, output, only) = match args.as_slice() {
        [corpus, output, only @ ..] => (corpus, output, only),
        _ => {
            eprintln!("usage : precision <corpus> <sortie.jsonl> [dépôt…]");
            std::process::exit(2);
        }
    };
    if only.is_empty() {
        return supervise(corpus, output);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime Tokio");
    // `HERALD_PRECISION_BASELINE=1` : règles du moteur seules, sans filtre ni
    // règles de l'app (mesure de référence).
    let keep_all = std::env::var_os("HERALD_PRECISION_BASELINE").is_some();
    let sources: Vec<String> = if keep_all {
        herald_engine::dsl::builtin_rules()
            .into_iter()
            .map(|rule| rule.source.to_owned())
            .collect()
    } else {
        local_rules::builtin_sources()
    };
    // Dépôts nommés : ajoutés à la sortie existante (reprise après un arrêt).
    let mut out = fs::OpenOptions::new()
        .create(true)
        .append(!only.is_empty())
        .write(true)
        .truncate(only.is_empty())
        .open(output)
        .expect("fichier de sortie");

    let mut repos: Vec<_> = fs::read_dir(corpus)
        .expect("dossier du corpus")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(".git").exists())
        .filter(|path| {
            only.is_empty()
                || path
                    .file_name()
                    .is_some_and(|name| only.iter().any(|o| o.as_str() == name))
        })
        .collect();
    repos.sort();

    let mut per_rule: BTreeMap<String, usize> = BTreeMap::new();
    for repo in &repos {
        let name = repo.file_name().unwrap().to_string_lossy().into_owned();
        eprint!("{name:<24} ");
        let commit = head_commit(repo).unwrap_or_default();
        let files: Vec<_> = collect::collect_files(repo, collect::DEFAULT_MAX_FILES, |_, _| {})
            .files
            .into_iter()
            .filter(|file| !manifest::is_manifest(&file.path))
            .collect();
        let config = Config {
            rule_sources: sources.clone(),
            ..Config::default()
        };
        let request = AnalyzeRequest { files, git: None };
        let response = runtime.block_on(herald_engine::analyze(&request, &config));
        let code: BTreeMap<&str, &str> = request
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.code.as_str()))
            .collect();

        let mut found = 0;
        for file in &response.files {
            let source = code.get(file.path.as_str()).copied().unwrap_or_default();
            for issue in file.issues.iter().filter(|i| {
                i.category == Category::Security
                    && (keep_all || !local_rules::dismissed(&i.rule, &file.path, source))
            }) {
                let snippet = code
                    .get(file.path.as_str())
                    .map(|code| snippet(code, issue.line))
                    .unwrap_or_default();
                let record = serde_json::json!({
                    "repo": name,
                    "commit": commit,
                    "path": file.path,
                    "line": issue.line,
                    "rule": issue.rule,
                    "severity": format!("{:?}", issue.severity),
                    "snippet": snippet,
                });
                writeln!(out, "{record}").expect("écriture");
                *per_rule.entry(issue.rule.clone()).or_default() += 1;
                found += 1;
            }
        }
        eprintln!("{:>5} fichiers {found:>5} relevés", request.files.len());
    }
    eprintln!("\nRelevés par règle :");
    for (rule, count) in per_rule {
        eprintln!("{count:>6}  {rule}");
    }
}

/// Délai par dépôt : au-delà, l'analyse est considérée comme bloquée.
const REPO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Analyse chaque dépôt dans un sous-processus (un dépôt qui bloque le moteur
/// n'arrête pas la mesure), puis résume les relevés par règle.
fn supervise(corpus: &str, output: &str) {
    fs::write(output, "").expect("fichier de sortie");
    let mut repos: Vec<String> = fs::read_dir(corpus)
        .expect("dossier du corpus")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(".git").exists())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    repos.sort();
    let exe = std::env::current_exe().expect("exécutable");
    let mut blocked = Vec::new();
    for repo in &repos {
        let mut child = std::process::Command::new(&exe)
            .args([corpus, output, repo])
            .spawn()
            .expect("sous-processus");
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        eprintln!("échec ({status})");
                        blocked.push(format!("{repo} (échec)"));
                    }
                    break;
                }
                _ if started.elapsed() > REPO_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("bloqué après {} s", REPO_TIMEOUT.as_secs());
                    blocked.push(format!("{repo} (bloqué)"));
                    break;
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
    }
    let mut per_rule: BTreeMap<String, usize> = BTreeMap::new();
    for line in fs::read_to_string(output).unwrap_or_default().lines() {
        if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
            let rule = record["rule"].as_str().unwrap_or_default().to_owned();
            *per_rule.entry(rule).or_default() += 1;
        }
    }
    eprintln!("\nRelevés par règle :");
    for (rule, count) in per_rule {
        eprintln!("{count:>6}  {rule}");
    }
    if !blocked.is_empty() {
        eprintln!("\nDépôts non mesurés : {}", blocked.join(", "));
    }
}

/// Lignes autour du relevé, préfixées de leur numéro (`>` sur la ligne visée).
fn snippet(code: &str, line: usize) -> Vec<String> {
    let lines: Vec<&str> = code.lines().collect();
    let start = line.saturating_sub(1 + CONTEXT_LINES);
    let end = (line + CONTEXT_LINES).min(lines.len());
    (start..end)
        .map(|ix| {
            let text: String = lines[ix].chars().take(MAX_LINE_CHARS).collect();
            let mark = if ix + 1 == line { '>' } else { ' ' };
            format!("{mark}{:>5} {}", ix + 1, text.trim_end())
        })
        .collect()
}

/// Commit du clone, lu dans `.git` (pas besoin de lancer git).
fn head_commit(repo: &Path) -> Option<String> {
    let head = fs::read_to_string(repo.join(".git/HEAD")).ok()?;
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return Some(head.trim().to_owned());
    };
    if let Ok(sha) = fs::read_to_string(repo.join(".git").join(reference)) {
        return Some(sha.trim().to_owned());
    }
    fs::read_to_string(repo.join(".git/packed-refs"))
        .ok()?
        .lines()
        .find(|line| line.ends_with(reference))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
}
