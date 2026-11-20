use crate::helpers::ast_query;
use crate::language::Language;
use crate::category::Category;
use crate::types::{FnRule, Issue, Severity};
use tree_sitter::Tree;

pub const META: FnRule = FnRule {
    name: "todo_fixme",
    category: Category::Quality,
    check,
};

// Marqueurs de commentaire : ce sont des « code-smells », jamais des erreurs
// critiques. FIXME est un cran au-dessus (à traiter), mais reste un warning.
const MARKERS: &[(&str, Severity)] = &[
    ("FIXME", Severity::Warning),
    ("TODO",  Severity::Info),
    ("XXX",   Severity::Info),
    ("HACK",  Severity::Info),
];

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    ast_query(tree, code, language, "(comment) @c", find_markers)
}

fn find_markers(text: &str, line: usize, column: usize) -> Vec<Issue> {
    let mut issues = Vec::new();

    for (marker, severity) in MARKERS {
        if text.contains(marker) {
            issues.push(Issue {
                rule: META.name.to_string(),
                category: Category::Quality,
                severity: severity.clone(),
                message: format!("Marqueur {marker} laissé dans un commentaire — traite-le ou ouvre un ticket"),
                line,
                column,
            });
        }
    }

    issues
}
