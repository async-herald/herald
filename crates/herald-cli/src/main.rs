//! CLI Herald : analyse locale de code avec le moteur open source `herald-engine`.
//!
//!   herald [chemin]          analyse un fichier ou un dossier (défaut : ".")
//!   herald [chemin] --json   sortie JSON

use std::fs;
use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use herald_engine::config::Config;
use herald_engine::dependencies::manifest;
use herald_engine::language::Language;
use herald_engine::types::{AnalyzeRequest, FileInput, Severity};

const WIDTH: usize = 76;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let as_json = args.iter().any(|a| a == "--json");
    let target = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| ".".to_string());

    let config = Config::load(Path::new("."));
    let files = collect_files(&target, &config);
    if files.is_empty() {
        eprintln!("Aucun fichier supporté à : {target}");
        std::process::exit(1);
    }

    let request = AnalyzeRequest { files, git: None };
    let response = herald_engine::analyze(&request, &config).await;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&response).unwrap());
    } else {
        print_human(&response);
    }

    if response.score.errors > 0 {
        std::process::exit(1);
    }
}

fn collect_files(path: &str, config: &Config) -> Vec<FileInput> {
    let p = Path::new(path);
    if p.is_file() {
        return read_file(p).into_iter().collect();
    }
    WalkDir::new(p)
        .into_iter()
        .filter_entry(|e| !is_hidden(e) && !is_ignored(e.path(), &config.ignore))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let path = e.path();
            let code = if is_supported(path) || is_manifest(path) {
                fs::read_to_string(path).unwrap_or_default()
            } else {
                String::new()
            };
            FileInput {
                path: path.display().to_string(),
                code,
            }
        })
        .collect()
}

fn is_supported(p: &Path) -> bool {
    Language::from_path(&p.display().to_string()).is_some()
}

fn is_manifest(p: &Path) -> bool {
    p.to_str().is_some_and(manifest::is_manifest)
}

fn is_ignored(path: &Path, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        let pat = pat.trim_matches('/');
        !pat.is_empty() && path.components().any(|c| c.as_os_str() == pat)
    })
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .map(|s| s != "." && s.starts_with('.'))
            .unwrap_or(false)
}

fn read_file(p: &Path) -> Option<FileInput> {
    let code = fs::read_to_string(p).ok()?;
    Some(FileInput {
        path: p.display().to_string(),
        code,
    })
}

fn severity_rank(s: &Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn print_human(response: &herald_engine::types::AnalyzeResponse) {
    use std::collections::BTreeMap;

    let mut by_category: BTreeMap<&str, Vec<(&str, &herald_engine::types::Issue)>> = BTreeMap::new();
    for file in &response.files {
        for issue in &file.issues {
            by_category
                .entry(issue.category.name())
                .or_default()
                .push((&file.path, issue));
        }
    }

    println!();
    for (cat_name, mut items) in by_category {
        let prefix = format!("== {} ({}) ", cat_name.to_uppercase(), items.len());
        let dashes = "=".repeat(WIDTH.saturating_sub(prefix.chars().count()));
        println!("{prefix}{dashes}\n");

        items.sort_by(|a, b| {
            severity_rank(&a.1.severity)
                .cmp(&severity_rank(&b.1.severity))
                .then(a.0.cmp(b.0))
                .then(a.1.line.cmp(&b.1.line))
        });
        for (file_path, issue) in items {
            let icon = match issue.severity {
                Severity::Error => "x",
                Severity::Warning => "!",
                Severity::Info => "i",
            };
            let location = format!("{}:{}:{}", file_path, issue.line, issue.column);
            println!("  {icon} {location:<40}  {:<40}  {}", issue.message, issue.rule);
        }
        println!();
    }

    let s = &response.score;
    let marker = if s.errors > 0 {
        "x"
    } else if response.total_issues > 0 {
        "!"
    } else {
        "ok"
    };
    println!(
        "  {marker} {} problème(s)  ({} erreurs, {} avertissements, {} infos)  Score {}/100 ({:?})",
        response.total_issues, s.errors, s.warnings, s.infos, s.value, s.grade
    );
    println!();
}
