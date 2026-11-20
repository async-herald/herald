//! Quadratic patterns: a linear search (`.includes`/`.indexOf`/`in_array`)
//! repeated inside a loop. Nested functions reset the depth (a callback isn't
//! the same iteration space).
//!
//! Used to also flag ANY syntactically nested loop as "O(n²)", but most real
//! nested loops iterate a small, bounded inner collection (e.g. splitting a
//! key into parts, formatting a few debug fields) that doesn't scale with the
//! outer loop — that blanket flag fired constantly on well-designed code
//! (33 hits on Flask alone) without any real complexity signal. Only the
//! linear-search-in-a-loop pattern below is a reliable enough signal to keep.

use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Severity};

pub fn check(code: &str, tree: Option<&Tree>, _lang: Language) -> Vec<Issue> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit(tree.root_node(), 0, code, &mut out);
    out
}

fn is_loop(k: &str) -> bool {
    matches!(
        k,
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement"
            | "foreach_statement" // PHP
    )
}

fn is_fn(k: &str) -> bool {
    matches!(
        k,
        "function_declaration" | "function_expression" | "function" | "arrow_function"
            | "method_definition" | "generator_function" | "generator_function_declaration"
            | "function_definition" | "lambda"
            | "method_declaration" | "anonymous_function" // PHP
    )
}

fn flag(node: Node, msg: &str, out: &mut Vec<Issue>) {
    let p = node.start_position();
    out.push(Issue {
        rule: "quadratic-loop".into(),
        category: Category::Quality,
        severity: Severity::Warning,
        message: msg.into(),
        line: p.row + 1,
        column: p.column + 1,
    });
}

fn visit(node: Node, depth: usize, code: &str, out: &mut Vec<Issue>) {
    let kind = node.kind();
    if depth >= 1
        && matches!(
            kind,
            "call_expression" | "call" | "function_call_expression" | "member_call_expression"
        )
    {
        let text = node.utf8_text(code.as_bytes()).unwrap_or("");
        if text.contains(".includes(") || text.contains(".indexOf(") {
            flag(node, "Recherche linéaire dans une boucle (.includes/.indexOf, O(n²)) — utilise un Set", out);
        } else if text.starts_with("in_array(") || text.starts_with("array_search(") {
            flag(node, "Recherche linéaire dans une boucle (in_array/array_search, O(n²)) — indexe le tableau une fois (array_flip) et teste avec isset()", out);
        }
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let child_depth = if is_fn(kind) {
                0
            } else if is_loop(kind) {
                if cursor.field_name() == Some("body") { depth + 1 } else { depth }
            } else {
                depth
            };
            visit(child, child_depth, code, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}
