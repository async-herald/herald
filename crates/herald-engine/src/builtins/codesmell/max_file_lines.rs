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
        category: Category::CodeSmell,
        severity: Severity::Warning,
        message: format!("File too long: {lines} lines (max {max})"),
        line: 1,
        column: 1,
    }]
}
