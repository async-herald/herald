//! Function-level code metrics: length, parameter count, cyclomatic complexity.
//!
//! All three share one principle for avoiding false positives: count *logical*
//! units (not raw text), respect each language's idioms (an options object is
//! one param, Go groups params, `self`/`this` don't count, Rust's `?` is not a
//! branch), and use per-language thresholds since verbosity differs a lot.

use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Severity};

pub const FN_LENGTH_NAME: &str = "function-too-long";
pub const PARAMS_NAME: &str = "too-many-params";
pub const COMPLEXITY_NAME: &str = "high-complexity";
pub const NESTING_NAME: &str = "deep-nesting";
pub const GOD_CLASS_NAME: &str = "god-class";

// ---- per-language defaults (used when the config threshold is 0 = "auto") ----

fn default_max_lines(lang: Language) -> usize {
    match lang {
        Language::Python | Language::JavaScript | Language::TypeScript => 50,
        Language::Go | Language::Rust => 60,
        Language::Java => 70, // boilerplate-heavy
        _ => 50,
    }
}

fn default_max_params(lang: Language) -> usize {
    match lang {
        Language::JavaScript | Language::TypeScript => 4, // options-object idiom
        Language::Python | Language::Go | Language::Rust => 5,
        Language::Java => 7, // DI constructors
        _ => 5,
    }
}

fn default_max_complexity(lang: Language) -> usize {
    match lang {
        Language::JavaScript | Language::TypeScript | Language::Python => 10,
        Language::Java | Language::Go => 12,
        Language::Rust => 15, // match-heavy style
        _ => 10,
    }
}

fn default_max_nesting(_lang: Language) -> usize {
    4
}

fn default_max_methods(lang: Language) -> usize {
    match lang {
        Language::Java => 25, // boilerplate (getters/setters)
        _ => 20,
    }
}

// ---- function discovery ----

fn is_function_kind(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => matches!(
            kind,
            "function_declaration"
                | "function_expression"
                | "function"
                | "arrow_function"
                | "method_definition"
                | "generator_function"
                | "generator_function_declaration"
        ),
        Language::Python => kind == "function_definition",
        Language::Java => matches!(
            kind,
            "method_declaration" | "constructor_declaration" | "lambda_expression"
        ),
        Language::Go => matches!(
            kind,
            "function_declaration" | "method_declaration" | "func_literal"
        ),
        Language::Rust => matches!(kind, "function_item" | "closure_expression"),
        _ => false,
    }
}

fn collect_functions<'a>(node: Node<'a>, lang: Language, out: &mut Vec<Node<'a>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_function_kind(lang, child.kind()) {
            out.push(child);
        }
        collect_functions(child, lang, out);
    }
}

fn functions(tree: Option<&Tree>, lang: Language) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    if let Some(tree) = tree {
        collect_functions(tree.root_node(), lang, &mut out);
    }
    out
}

fn issue(name: &str, message: String, node: Node) -> Issue {
    let p = node.start_position();
    Issue {
        rule: name.to_string(),
        category: Category::CodeSmell,
        severity: Severity::Warning,
        message,
        line: p.row + 1,
        column: p.column + 1,
    }
}

// ---- 1. function length (logical lines) ----

pub fn function_length(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let max = if override_max == 0 {
        default_max_lines(lang)
    } else {
        override_max
    };
    let lines: Vec<&str> = code.lines().collect();

    functions(tree, lang)
        .into_iter()
        .filter_map(|func| {
            let n = logical_lines(func, &lines);
            (n > max).then(|| {
                issue(
                    FN_LENGTH_NAME,
                    format!("Function is too long: {n} logical lines (max {max}) — split it into smaller units"),
                    func,
                )
            })
        })
        .collect()
}

fn logical_lines(func: Node, lines: &[&str]) -> usize {
    let start = func.start_position().row;
    let end = func.end_position().row;
    (start..=end)
        .filter_map(|i| lines.get(i))
        .filter(|line| {
            let t = line.trim_start();
            !t.is_empty()
                && !t.starts_with("//")
                && !t.starts_with('#')
                && !t.starts_with("*")
                && !t.starts_with("/*")
        })
        .count()
}

// ---- 2. parameter count ----

pub fn param_count(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let max = if override_max == 0 {
        default_max_params(lang)
    } else {
        override_max
    };

    functions(tree, lang)
        .into_iter()
        .filter_map(|func| {
            let n = count_params(func, lang, code);
            (n > max).then(|| {
                issue(
                    PARAMS_NAME,
                    format!("Too many parameters: {n} (max {max}) — group related arguments into an object/struct"),
                    func,
                )
            })
        })
        .collect()
}

fn params_node<'a>(func: Node<'a>) -> Option<Node<'a>> {
    if let Some(p) = func.child_by_field_name("parameters") {
        return Some(p);
    }
    let mut c = func.walk();
    func.children(&mut c).find(|ch| {
        matches!(
            ch.kind(),
            "formal_parameters" | "parameters" | "parameter_list" | "closure_parameters"
        )
    })
}

fn count_params(func: Node, lang: Language, code: &str) -> usize {
    let Some(pnode) = params_node(func) else {
        // unparenthesized arrow: `x => …`
        if func.kind() == "arrow_function" && func.child_by_field_name("parameter").is_some() {
            return 1;
        }
        return 0;
    };

    // Go groups names: `func f(a, b int)` is one parameter_declaration / 2 params.
    if lang == Language::Go {
        let mut total = 0;
        let mut c = pnode.walk();
        for decl in pnode.named_children(&mut c) {
            if decl.kind() == "comment" {
                continue;
            }
            let mut dc = decl.walk();
            let names = decl
                .named_children(&mut dc)
                .filter(|n| n.kind() == "identifier")
                .count();
            total += names.max(1);
        }
        return total;
    }

    let mut c = pnode.walk();
    pnode
        .named_children(&mut c)
        .filter(|ch| {
            let k = ch.kind();
            if k == "comment" || k == "self_parameter" {
                return false;
            }
            let t = ch.utf8_text(code.as_bytes()).unwrap_or("").trim();
            !(t == "self" || t == "cls" || t == "this" || t.starts_with("this:") || t.starts_with("self:"))
        })
        .count()
}

// ---- 3. cyclomatic complexity ----

pub fn complexity(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let max = if override_max == 0 {
        default_max_complexity(lang)
    } else {
        override_max
    };

    functions(tree, lang)
        .into_iter()
        .filter_map(|func| {
            let n = function_complexity(func, lang, code);
            (n > max).then(|| {
                issue(
                    COMPLEXITY_NAME,
                    format!("High cyclomatic complexity: {n} (max {max}) — simplify branching or extract helpers"),
                    func,
                )
            })
        })
        .collect()
}

fn function_complexity(func: Node, lang: Language, code: &str) -> usize {
    let mut count = 1usize; // base path
    count_decisions(func, lang, code, &mut count);
    count
}

fn count_decisions(node: Node, lang: Language, code: &str, count: &mut usize) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // A nested function carries its own complexity — don't descend into it.
        if is_function_kind(lang, child.kind()) {
            continue;
        }
        if is_decision(lang, child, code) {
            *count += 1;
        }
        count_decisions(child, lang, code, count);
    }
}

fn is_decision(lang: Language, node: Node, code: &str) -> bool {
    let k = node.kind();

    let structural = match lang {
        Language::JavaScript | Language::TypeScript => matches!(
            k,
            "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "catch_clause"
                | "ternary_expression"
                | "switch_case" // not switch_default
        ),
        Language::Python => matches!(
            k,
            "if_statement"
                | "elif_clause"
                | "for_statement"
                | "while_statement"
                | "except_clause"
                | "conditional_expression"
                | "case_clause"
        ),
        Language::Java => matches!(
            k,
            "if_statement"
                | "for_statement"
                | "enhanced_for_statement"
                | "while_statement"
                | "do_statement"
                | "catch_clause"
                | "ternary_expression"
        ),
        Language::Go => matches!(
            k,
            "if_statement"
                | "for_statement"
                | "expression_case"
                | "type_case"
                | "communication_case"
        ),
        Language::Rust => matches!(
            k,
            "if_expression" | "while_expression" | "for_expression" | "loop_expression" | "match_arm"
            // note: `?` (try_expression) intentionally NOT counted — idiomatic in Rust
        ),
        _ => false,
    };
    if structural {
        return true;
    }

    // Java `case X:` is a switch_label; exclude `default:`.
    if lang == Language::Java && k == "switch_label" {
        let t = node.utf8_text(code.as_bytes()).unwrap_or("");
        return t.trim_start().starts_with("case");
    }

    // Short-circuit boolean operators add a path.
    match lang {
        Language::Python => k == "boolean_operator",
        _ => {
            if k == "binary_expression"
                && let Some(op) = node.child_by_field_name("operator") {
                    let t = op.utf8_text(code.as_bytes()).unwrap_or("");
                    return t == "&&" || t == "||" || t == "??";
                }
            false
        }
    }
}

// ---- 4. nesting depth ----

pub fn nesting(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let _ = code;
    let max = if override_max == 0 {
        default_max_nesting(lang)
    } else {
        override_max
    };

    functions(tree, lang)
        .into_iter()
        .filter_map(|func| {
            let d = max_depth(func, lang, 0);
            (d > max).then(|| {
                issue(
                    NESTING_NAME,
                    format!("Nesting too deep: {d} levels (max {max}) — extract helpers or use early returns / guard clauses"),
                    func,
                )
            })
        })
        .collect()
}

fn is_nesting(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "try_statement"
        ),
        Language::Python => matches!(
            kind,
            "if_statement" | "for_statement" | "while_statement" | "with_statement" | "try_statement"
        ),
        Language::Java => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "enhanced_for_statement"
                | "while_statement"
                | "do_statement"
                | "switch_expression"
                | "switch_statement"
                | "try_statement"
        ),
        Language::Go => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "expression_switch_statement"
                | "type_switch_statement"
                | "select_statement"
        ),
        Language::Rust => matches!(
            kind,
            "if_expression" | "for_expression" | "while_expression" | "loop_expression" | "match_expression"
        ),
        _ => false,
    }
}

// `else if` is structurally a nested if, but it's the same logical level —
// don't let it inflate the depth.
fn is_else_if(node: Node) -> bool {
    if !matches!(node.kind(), "if_statement" | "if_expression") {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "else_clause" => true, // JS/TS/Python
        "if_statement" | "if_expression" => parent
            .child_by_field_name("alternative")
            .is_some_and(|a| a.id() == node.id()), // Java/Go/Rust
        _ => false,
    }
}

fn max_depth(node: Node, lang: Language, current: usize) -> usize {
    let mut max = current;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_function_kind(lang, child.kind()) {
            continue; // nested function carries its own depth
        }
        let next = if is_nesting(lang, child.kind()) && !is_else_if(child) {
            current + 1
        } else {
            current
        };
        max = max.max(max_depth(child, lang, next));
    }
    max
}

// ---- 5. god class (too many methods) ----

pub fn god_class(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let _ = code;
    let max = if override_max == 0 {
        default_max_methods(lang)
    } else {
        override_max
    };

    let mut classes = Vec::new();
    if let Some(tree) = tree {
        collect_classes(tree.root_node(), lang, &mut classes);
    }

    classes
        .into_iter()
        .filter_map(|cls| {
            let n = count_methods(cls, lang);
            (n > max).then(|| {
                issue(
                    GOD_CLASS_NAME,
                    format!("God class: {n} methods (max {max}) — split responsibilities into smaller, focused classes"),
                    cls,
                )
            })
        })
        .collect()
}

fn is_class_kind(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => matches!(kind, "class_declaration" | "class"),
        Language::Python => kind == "class_definition",
        Language::Java => kind == "class_declaration",
        Language::Go | Language::Rust => false,
        _ => false,
    }
}

fn is_method_kind(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => kind == "method_definition",
        Language::Python => kind == "function_definition",
        Language::Java => matches!(kind, "method_declaration" | "constructor_declaration"),
        Language::Go | Language::Rust => false,
        _ => false,
    }
}

fn collect_classes<'a>(node: Node<'a>, lang: Language, out: &mut Vec<Node<'a>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_class_kind(lang, child.kind()) {
            out.push(child);
        }
        collect_classes(child, lang, out);
    }
}

fn count_methods(class: Node, lang: Language) -> usize {
    let Some(body) = class.child_by_field_name("body") else {
        return 0;
    };
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|n| is_method_kind(lang, n.kind()))
        .count()
}
