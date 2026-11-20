//! Leftover git merge-conflict markers in source.

use tree_sitter::Tree;

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Severity};

/// Flags lines that begin a conflict hunk (`<<<<<<< `, `>>>>>>> `) or the exact
/// `=======` separator. Specific enough to avoid comment-banner false positives.
pub fn check(code: &str, _tree: Option<&Tree>, _lang: Language) -> Vec<Issue> {
    let mut out = Vec::new();
    for (i, line) in code.lines().enumerate() {
        let is_marker = line.starts_with("<<<<<<< ")
            || line.starts_with(">>>>>>> ")
            || line == "=======";
        if is_marker {
            out.push(Issue {
                rule: "merge-conflict".to_string(),
                category: Category::Git,
                severity: Severity::Error,
                message: "Marqueur de conflit de merge laissé dans le code".to_string(),
                line: i + 1,
                column: 1,
            });
        }
    }
    out
}
