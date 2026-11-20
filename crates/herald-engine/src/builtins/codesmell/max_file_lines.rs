use crate::category::Category;
use crate::types::{Issue, Severity};

pub const NAME: &str = "max_file_lines";

pub fn check(code: &str, max: usize) -> Vec<Issue> {
    let lines = code.lines().count();
    if lines <= max {
        return Vec::new();
    }

    vec![Issue {
        rule: NAME.to_string(),
        category: Category::Quality,
        severity: Severity::Warning,
        message: format!("Fichier trop long : {lines} lignes (max {max}) — découpe-le en modules plus petits"),
        line: 1,
        column: 1,
    }]
}
