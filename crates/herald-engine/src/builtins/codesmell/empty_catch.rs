//! Empty exception handler — `catch`/`except` that swallows the error.

use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "empty-catch",
    category: Category::CodeSmell,
    check,
};

fn catch_kind(language: Language) -> Option<&'static str> {
    match language {
        Language::JavaScript | Language::TypeScript | Language::Java => Some("catch_clause"),
        Language::Python => Some("except_clause"),
        Language::Go | Language::Rust => None, // no try/catch
        _ => None,
    }
}

// A "real" handler statement (anything that actually deals with the error).
fn is_real(language: Language, kind: &str) -> bool {
    if matches!(kind, "comment" | "line_comment" | "block_comment") {
        return false;
    }
    if language == Language::Python && kind == "pass_statement" {
        return false;
    }
    true
}

fn body_of<'a>(catch: Node<'a>) -> Option<Node<'a>> {
    if let Some(b) = catch.child_by_field_name("body") {
        return Some(b);
    }
    let mut c = catch.walk();
    catch
        .children(&mut c)
        .find(|n| matches!(n.kind(), "block" | "statement_block"))
}

fn walk(node: Node, language: Language, catch: &str, out: &mut Vec<Issue>) {
    if node.kind() == catch
        && let Some(body) = body_of(node) {
            let mut c = body.walk();
            let real = body
                .named_children(&mut c)
                .filter(|n| is_real(language, n.kind()))
                .count();
            if real == 0 {
                let p = node.start_position();
                out.push(Issue {
                    rule: META.name.to_string(),
                    category: Category::CodeSmell,
                    severity: Severity::Warning,
                    message: "Empty catch block — the error is swallowed silently; log it, handle it, or rethrow".to_string(),
                    line: p.row + 1,
                    column: p.column + 1,
                });
            }
        }

    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, language, catch, out);
    }
}

pub fn check(_code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let (Some(tree), Some(catch)) = (tree, catch_kind(language)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(tree.root_node(), language, catch, &mut out);
    out
}
