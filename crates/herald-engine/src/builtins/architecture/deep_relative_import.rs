use std::sync::OnceLock;

use regex::Regex;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};
use tree_sitter::Tree;

pub const META: FnRule = FnRule {
    name: "deep-relative-import",
    category: Category::Pattern,
    check,
};

fn deep_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\.\./){3,}").expect("deep-relative-import regex"))
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    if !matches!(language, Language::JavaScript | Language::TypeScript) {
        return Vec::new();
    }
    ast_query(
        tree,
        code,
        language,
        "(import_statement source: (string) @s)",
        |text, line, column| {
            if deep_re().is_match(text) {
                vec![Issue {
                    rule: META.name.to_string(),
                    category: Category::Pattern,
                    severity: Severity::Warning,
                    message: format!(
                        "Import relatif profond {text} — franchit plusieurs niveaux ; préférer un alias / point d'entrée de module"
                    ),
                    line,
                    column,
                }]
            } else {
                Vec::new()
            }
        },
    )
}
