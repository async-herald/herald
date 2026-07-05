use tree_sitter::Tree;

use crate::{category::Category, config::Config, dsl, language::Language, types::Issue};

pub fn run_all(
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
    config: &Config,
) -> Vec<Issue> {
    dsl::run_all(code, tree, language, |id| config.is_rule_enabled(id))
}

pub fn run_context(
    path: Option<&str>,
    branch: Option<&str>,
    commit: Option<&str>,
    config: &Config,
) -> Vec<Issue> {
    dsl::run_context(path, branch, commit, |id| config.is_rule_enabled(id))
}

pub fn list() -> Vec<(String, String)> {
    let extra = [
        ("import-cycle", Category::Architecture.name()),
        ("orphan-module", Category::CodeSmell.name()),
        ("unused-export", Category::CodeSmell.name()),
        ("committed-env-file", Category::Security.name()),
        ("committed-secret-file", Category::Security.name()),
        ("private-key-block", Category::Security.name()),
        ("multiple-lockfiles", Category::Git.name()),
        ("build-output-committed", Category::Git.name()),
    ]
    .into_iter()
    .map(|(n, c)| (n.to_string(), c.to_string()));

    let mut all: Vec<(String, String)> = dsl::rule_list().into_iter().chain(extra).collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}
