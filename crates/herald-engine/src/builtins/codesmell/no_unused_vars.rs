use std::collections::HashMap;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};
use tree_sitter::Tree;

pub const META: FnRule = FnRule {
    name: "no_unused_vars",
    category: Category::CodeSmell,
    check,
};

fn decl_query(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::JavaScript | Language::TypeScript =>
            "(variable_declarator name: (identifier) @decl)",
        Language::Rust =>
            "(let_declaration pattern: (identifier) @decl)",
        _ => return None,
    })
}

fn ref_query(language: Language) -> &'static str {
    match language {
        Language::JavaScript | Language::TypeScript =>
            "[(identifier) (shorthand_property_identifier)] @i",
        _ => "(identifier) @i",
    }
}

/// Noms déclarés *et exportés* : jamais « inutilisés », ils font partie de l'API
/// publique du module. En JS/TS, `export const metadata` / `viewport` / `runtime`
/// / `alt` / `contentType` sont consommés par le framework, pas dans le fichier.
fn exported_query(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::JavaScript | Language::TypeScript =>
            "(export_statement (lexical_declaration (variable_declarator name: (identifier) @exp))) \
             (export_statement (variable_declaration (variable_declarator name: (identifier) @exp)))",
        _ => return None,
    })
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let Some(decls) = decl_query(language) else { return Vec::new() };

    let mut freq: HashMap<String, usize> = HashMap::new();
    ast_query(tree, code, language, ref_query(language), |text, _, _| {
        *freq.entry(text.to_string()).or_default() += 1;
        Vec::new()
    });

    let mut exported: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(q) = exported_query(language) {
        ast_query(tree, code, language, q, |name, _, _| {
            exported.insert(name.to_string());
            Vec::new()
        });
    }

    ast_query(tree, code, language, decls, |name, line, column| {
        if name.starts_with('_') || exported.contains(name) || freq.get(name).copied().unwrap_or(0) > 1 {
            return Vec::new();
        }
        vec![Issue {
            rule: META.name.to_string(),
            category: Category::CodeSmell,
            severity: Severity::Warning,
            message: format!("Unused variable '{name}' — remove it or prefix with '_'"),
            line,
            column,
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Tree;

    fn count(code: &str, lang: Language) -> usize {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&lang.tree_sitter()).unwrap();
        let t: Tree = p.parse(code, None).unwrap();
        check(code, Some(&t), lang).len()
    }

    #[test]
    fn flags_truly_unused_local() {
        assert_eq!(count("const x = 1;\n", Language::TypeScript), 1);
    }

    #[test]
    fn skips_exported_bindings() {
        // Exports Next.js : consommés par le framework, jamais « inutilisés ».
        assert_eq!(count("export const metadata = { title: 'a' };\n", Language::TypeScript), 0);
        assert_eq!(count("export const runtime = 'edge';\n", Language::TypeScript), 0);
        assert_eq!(count("export const viewport = { width: 1 };\n", Language::TypeScript), 0);
    }

    #[test]
    fn skips_underscore_and_used() {
        assert_eq!(count("const _x = 1;\n", Language::TypeScript), 0);
        assert_eq!(count("const x = 1; console.log(x);\n", Language::TypeScript), 0);
    }
}