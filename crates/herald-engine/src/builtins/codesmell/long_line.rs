use crate::category::Category;
use crate::types::{Issue, Severity};

pub const NAME: &str = "long-line";

pub fn check(code: &str, max: usize) -> Vec<Issue> {
    if max == 0 {
        return Vec::new();
    }

    code.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let len = line.chars().count();
            // Skip lines whose length comes from an unsplittable URL.
            if len <= max || line.contains("http://") || line.contains("https://") {
                return None;
            }
            Some(Issue {
                rule: NAME.to_string(),
                category: Category::CodeSmell,
                severity: Severity::Warning,
                message: format!("Line too long: {len} chars (max {max})"),
                line: i + 1,
                column: max + 1,
            })
        })
        .collect()
}
