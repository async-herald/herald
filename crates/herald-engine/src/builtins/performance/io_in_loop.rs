//! Heavy I/O inside a loop body — the classic N+1 / serialised-await problem.
//! Flags `await` (js/ts) and network/DB calls (fetch/axios/requests/.query/
//! .execute/.aggregate) that run once per iteration instead of being batched
//! or parallelised. Nested functions are a new boundary (not counted).

use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Severity};

static IO_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?x)
        ^ (fetch|axios|urlopen) \s*\(
        | ^ requests \. \w+ \s*\(
        | (\.|->) (query|execute|executemany|executeQuery|aggregate|fetchall|fetchone|fetchAll|fetchAssoc) \s*\(
        | ^ (curl_exec|file_get_contents|mysqli_query|pg_query) \s*\(
    ").unwrap()
});

pub fn check(code: &str, tree: Option<&Tree>, _lang: Language) -> Vec<Issue> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    let mut out = Vec::new();
    visit(tree.root_node(), false, code, &mut out);
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

fn flag(node: Node, out: &mut Vec<Issue>) {
    let p = node.start_position();
    out.push(Issue {
        rule: "io-in-loop".into(),
        category: Category::Quality,
        severity: Severity::Warning,
        message: "I/O dans une boucle (await / requête réseau ou DB) — sérialise les appels (N+1) ; batche-les ou parallélise (Promise.all)".into(),
        line: p.row + 1,
        column: p.column + 1,
    });
}

fn visit(node: Node, in_loop: bool, code: &str, out: &mut Vec<Issue>) {
    let kind = node.kind();
    if in_loop {
        if kind == "await_expression" {
            flag(node, out);
        } else if matches!(
            kind,
            "call_expression" | "call" | "function_call_expression" | "member_call_expression"
        ) {
            let text = node.utf8_text(code.as_bytes()).unwrap_or("");
            if IO_CALL.is_match(text) {
                flag(node, out);
            }
        }
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let child_in_loop = if is_fn(kind) {
                false
            } else if is_loop(kind) {
                cursor.field_name() == Some("body")
            } else {
                in_loop
            };
            visit(child, child_in_loop, code, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}
