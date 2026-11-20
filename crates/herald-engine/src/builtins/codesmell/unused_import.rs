use std::collections::HashMap;

use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "unused-import",
    category: Category::Quality,
    check,
};

// Every name node in the file — used to tell if an import is referenced.
// Type positions matter: Java/TS type names are `type_identifier`, not
// `identifier`, so a type-only usage would otherwise look unused.
fn ref_query(language: Language) -> &'static str {
    match language {
        Language::JavaScript => "[(identifier) (shorthand_property_identifier)] @i",
        Language::TypeScript => {
            "[(identifier) (shorthand_property_identifier) (type_identifier)] @i"
        }
        Language::Java => "[(identifier) (type_identifier)] @i",
        _ => "(identifier) @i",
    }
}

// Import binding names (the *local* name: alias when aliased).
fn import_query(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::JavaScript | Language::TypeScript => {
            r#"
            (import_clause (identifier) @decl)
            (namespace_import (identifier) @decl)
            (import_specifier alias: (identifier) @decl)
            (import_specifier !alias name: (identifier) @decl)
            "#
        }
        Language::Python => {
            r#"
            (aliased_import alias: (identifier) @decl)
            (import_from_statement name: (dotted_name . (identifier) @decl))
            (import_statement name: (dotted_name . (identifier) @decl))
            "#
        }
        Language::Java => r#"(import_declaration (scoped_identifier name: (identifier) @decl))"#,
        Language::Go | Language::Rust => return None, // compiler already errors
        _ => return None,
    })
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let Some(imports) = import_query(language) else {
        return Vec::new();
    };

    let mut freq: HashMap<String, usize> = HashMap::new();
    ast_query(tree, code, language, ref_query(language), |text, _, _| {
        *freq.entry(text.to_string()).or_default() += 1;
        Vec::new()
    });

    let lines: Vec<&str> = code.lines().collect();

    // Python/Java wildcard imports (`from m import *`, `import a.b.*`) bind
    // nothing trackable — skip those lines. JS `import * as ns` IS trackable.
    let skip_wildcard = matches!(language, Language::Python | Language::Java);

    ast_query(tree, code, language, imports, |name, line, column| {
        if skip_wildcard && lines.get(line - 1).map(|l| l.contains('*')).unwrap_or(false) {
            return Vec::new();
        }
        // freq counts the import binding itself once; >1 means it's referenced.
        if freq.get(name).copied().unwrap_or(0) > 1 {
            return Vec::new();
        }
        vec![Issue {
            rule: META.name.to_string(),
            category: Category::Quality,
            severity: Severity::Warning,
            message: format!("Import '{name}' inutilisé — supprime-le"),
            line,
            column,
        }]
    })
}
