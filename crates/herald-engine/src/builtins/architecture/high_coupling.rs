//! `high-coupling` — un module qui importe beaucoup d'autres modules est
//! fortement couplé (difficile à faire évoluer / tester en isolation). On compte
//! les déclarations d'import du fichier et on signale au-delà d'un seuil.

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};
use tree_sitter::Tree;

pub const META: FnRule = FnRule {
    name: "high-coupling",
    category: Category::Pattern,
    check,
};

/// Seuil d'imports au-delà duquel on parle de couplage fort.
const MAX_IMPORTS: usize = 20;

fn import_query(language: Language) -> Option<&'static str> {
    Some(match language {
        Language::JavaScript | Language::TypeScript => "(import_statement) @i",
        Language::Python => "[(import_statement) (import_from_statement)] @i",
        Language::Rust => "(use_declaration) @i",
        Language::Go => "(import_spec) @i",
        Language::Java => "(import_declaration) @i",
        _ => return None,
    })
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let Some(query) = import_query(language) else {
        return Vec::new();
    };

    let mut count = 0usize;
    ast_query(tree, code, language, query, |_, _, _| {
        count += 1;
        Vec::new()
    });

    if count > MAX_IMPORTS {
        vec![Issue {
            rule: META.name.to_string(),
            category: Category::Pattern,
            severity: Severity::Warning,
            message: format!(
                "Couplage élevé : {count} imports dans ce module (max conseillé {MAX_IMPORTS}) — envisager de le scinder"
            ),
            line: 1,
            column: 1,
        }]
    } else {
        Vec::new()
    }
}
