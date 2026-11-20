use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "commented-out-code",
    category: Category::Quality,
    check,
};

// Strong "this is code, not prose" signals. Keyword branches require an actual
// code-punctuation char so English sentences starting with if/for/return don't
// trigger.
static CODE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
          ;\s*$                                                         # ends with ;
        | ^[{}]                                                          # a brace line
        | \{\s*$                                                         # opens a block
        | ^(?:if|for|while|switch|else|elif|return|function|const|let|var|def|class|import|export|from|public|private|protected|static|void|func|fn|try|catch|except|raise|throw|new|await|async)\b.*[(){}=;:'"]
        | ^[A-Za-z_][\w.]*\s*=\s*[^=]                                    # assignment
        | ^[A-Za-z_][\w.]*\([^)]*\)\s*;?\s*$                             # bare call
        "#,
    )
    .unwrap()
});

fn looks_like_code(raw: &str) -> bool {
    let t = raw
        .trim()
        .trim_start_matches("///")
        .trim_start_matches("//")
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim_start_matches('*')
        .trim_start_matches('#')
        .trim();

    if t.len() < 4 || t.starts_with('@') || t.starts_with('!') {
        return false;
    }
    let lower = t.to_lowercase();
    for tag in ["todo", "fixme", "xxx", "hack", "note", "see ", "http"] {
        if lower.starts_with(tag) {
            return false;
        }
    }
    CODE_RE.is_match(t)
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    ast_query(tree, code, language, "(comment) @c", |text, line, column| {
        if text.lines().any(looks_like_code) {
            vec![Issue {
                rule: META.name.to_string(),
                category: Category::Quality,
                severity: Severity::Warning,
                message: "Code commenté laissé dans le fichier — supprime-le (Git garde l'historique)"
                    .to_string(),
                line,
                column,
            }]
        } else {
            Vec::new()
        }
    })
}
