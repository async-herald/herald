//! Per-language naming conventions. Deliberately conservative to avoid false
//! positives: only *unambiguous* violations are flagged. A single lowercase word
//! (`count`) is valid as both camelCase and snake_case, so it's never flagged.

use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "naming-conventions",
    category: Category::CodeSmell,
    check,
};

fn core(name: &str) -> &str {
    name.trim_matches('_')
}

fn is_dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

fn has_internal_underscore(name: &str) -> bool {
    core(name).contains('_')
}

fn has_upper(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase())
}

fn starts_lower(name: &str) -> bool {
    core(name)
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
}

fn flag(name: &str, what: &str, want: &str, line: usize, column: usize) -> Issue {
    Issue {
        rule: META.name.to_string(),
        category: Category::CodeSmell,
        severity: Severity::Warning,
        message: format!("Naming: {what} '{name}' should be {want}"),
        line,
        column,
    }
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    // (query, what, wanted, violates)
    type Rule = (&'static str, &'static str, &'static str, fn(&str) -> bool);

    let snake_func: fn(&str) -> bool = |n| !is_dunder(n) && has_upper(n);
    let camel_func: fn(&str) -> bool = |n| has_internal_underscore(n);
    let pascal_type: fn(&str) -> bool = |n| starts_lower(n);
    let go_mixed: fn(&str) -> bool = |n| has_internal_underscore(n);

    let rules: Vec<Rule> = match language {
        Language::Python => vec![
            ("(function_definition name: (identifier) @n)", "function", "snake_case", snake_func),
            ("(class_definition name: (identifier) @n)", "class", "PascalCase", pascal_type),
        ],
        Language::JavaScript => vec![
            ("(function_declaration name: (identifier) @n)", "function", "camelCase", camel_func),
            ("(method_definition name: (property_identifier) @n)", "method", "camelCase", camel_func),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type),
        ],
        Language::TypeScript => vec![
            ("(function_declaration name: (identifier) @n)", "function", "camelCase", camel_func),
            ("(method_definition name: (property_identifier) @n)", "method", "camelCase", camel_func),
            ("(class_declaration name: (type_identifier) @n)", "class", "PascalCase", pascal_type),
        ],
        Language::Java => vec![
            ("(method_declaration name: (identifier) @n)", "method", "camelCase", camel_func),
            ("(class_declaration name: (identifier) @n)", "class", "PascalCase", pascal_type),
            ("(interface_declaration name: (identifier) @n)", "interface", "PascalCase", pascal_type),
            ("(enum_declaration name: (identifier) @n)", "enum", "PascalCase", pascal_type),
        ],
        Language::Go => vec![
            ("(function_declaration name: (identifier) @n)", "function", "MixedCaps (no underscores)", go_mixed),
            ("(method_declaration name: (field_identifier) @n)", "method", "MixedCaps (no underscores)", go_mixed),
            ("(type_spec name: (type_identifier) @n)", "type", "MixedCaps (no underscores)", go_mixed),
        ],
        Language::Rust => Vec::new(), // rustc already lints non_snake_case / non_camel_case
        _ => Vec::new(),
    };

    let mut issues = Vec::new();
    for (query, what, want, violates) in rules {
        issues.extend(ast_query(tree, code, language, query, |name, line, column| {
            if name == "_" || core(name).len() < 2 {
                return Vec::new();
            }
            if violates(name) {
                vec![flag(name, what, want, line, column)]
            } else {
                Vec::new()
            }
        }));
    }

    issues.sort_by_key(|i| (i.line, i.column));
    issues.dedup_by_key(|i| (i.line, i.column));
    issues
}
