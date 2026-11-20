//! Code after an unconditional `return`/`throw`/`break`/`continue`.
//!
//! False-positive guards: we only look at *direct siblings inside the same
//! block* — a `return` nested in an `if` does NOT make the code after the `if`
//! unreachable. Hoisted declarations (JS function/class) and comments after the
//! terminator are not flagged. Rust is skipped (rustc already warns).

use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "unreachable-code",
    category: Category::Quality,
    check,
};

fn block_kind(language: Language) -> Option<&'static str> {
    match language {
        Language::JavaScript | Language::TypeScript => Some("statement_block"),
        Language::Python | Language::Java | Language::Go => Some("block"),
        Language::Rust => None,
        _ => None,
    }
}

fn is_terminator(language: Language, kind: &str) -> bool {
    let common = matches!(kind, "return_statement" | "break_statement" | "continue_statement");
    common
        || match language {
            Language::Python => kind == "raise_statement",
            Language::Go => kind == "goto_statement",
            _ => kind == "throw_statement",
        }
}

// Statements that follow a terminator but should NOT be flagged (hoisted in JS,
// or non-executable noise).
fn is_skippable(language: Language, kind: &str) -> bool {
    // Comment node kinds differ per grammar (Java: line_comment/block_comment).
    if matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "empty_statement"
    ) {
        return true;
    }
    matches!(language, Language::JavaScript | Language::TypeScript)
        && matches!(
            kind,
            "function_declaration" | "generator_function_declaration" | "class_declaration"
        )
}

fn walk(node: Node, language: Language, block: &str, code: &str, out: &mut Vec<Issue>) {
    if node.kind() == block {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        for (i, child) in children.iter().enumerate() {
            if is_terminator(language, child.kind()) {
                if let Some(next) = children[i + 1..]
                    .iter()
                    .find(|c| !is_skippable(language, c.kind()))
                {
                    let term = child.utf8_text(code.as_bytes()).unwrap_or("").trim();
                    let kw = term.split_whitespace().next().unwrap_or("return");
                    let p = next.start_position();
                    out.push(Issue {
                        rule: META.name.to_string(),
                        category: Category::Quality,
                        severity: Severity::Warning,
                        message: format!(
                            "Code mort après un `{kw}` inconditionnel — supprime-le"
                        ),
                        line: p.row + 1,
                        column: p.column + 1,
                    });
                }
                break; // one report per block is enough
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, language, block, code, out);
    }
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let (Some(tree), Some(block)) = (tree, block_kind(language)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(tree.root_node(), language, block, code, &mut out);
    out
}
