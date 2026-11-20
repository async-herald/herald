//! Function-level code metrics: length, parameter count, cyclomatic and
//! cognitive complexity, nesting depth, class size.
//!
//! They all share one principle for avoiding false positives: count *logical*
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
pub const COGNITIVE_NAME: &str = "high-cognitive-complexity";
pub const NESTING_NAME: &str = "deep-nesting";
pub const GOD_CLASS_NAME: &str = "god-class";

// ---- per-language defaults (used when the config threshold is 0 = "auto") ----

fn default_max_lines(lang: Language) -> usize {
    match lang {
        Language::Python | Language::JavaScript | Language::TypeScript => 50,
        Language::Go | Language::Rust => 60,
        Language::Java => 70, // boilerplate-heavy
        Language::Php => 60,  // type hints + docblocks stretch a method out
        _ => 50,
    }
}

fn default_max_params(lang: Language) -> usize {
    match lang {
        Language::JavaScript | Language::TypeScript => 4, // options-object idiom
        Language::Python | Language::Go | Language::Rust => 5,
        Language::Java => 7, // DI constructors
        Language::Php => 6,  // constructor promotion carries the DI deps
        _ => 5,
    }
}

fn default_max_complexity(lang: Language) -> usize {
    match lang {
        Language::JavaScript | Language::TypeScript | Language::Python => 10,
        Language::Java | Language::Go => 12,
        Language::Rust => 15, // match-heavy style
        Language::Php => 10,
        _ => 10,
    }
}

fn default_max_nesting(_lang: Language) -> usize {
    4
}

/// Cognitive complexity is about how hard code is to *follow*, so the threshold
/// doesn't vary with a language's verbosity. 15 is the usual industry default.
fn default_max_cognitive(_lang: Language) -> usize {
    15
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
        Language::Php => matches!(
            kind,
            "function_definition" | "method_declaration" | "anonymous_function" | "arrow_function"
        ),
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
        category: Category::Quality,
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
                    format!("Fonction trop longue : {n} lignes (max {max}) — découpe-la en fonctions plus petites"),
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
                    format!("Trop de paramètres : {n} (max {max}) — regroupe les arguments liés dans un objet/struct"),
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
                    format!("Complexité cyclomatique élevée : {n} (max {max}) — simplifie la logique ou extrais des fonctions"),
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
        // `elseif` is its own node (`else if` is an if_statement inside an
        // else_clause, already counted). `default:` / `default =>` add no branch.
        Language::Php => matches!(
            k,
            "if_statement"
                | "else_if_clause"
                | "for_statement"
                | "foreach_statement"
                | "while_statement"
                | "do_statement"
                | "catch_clause"
                | "conditional_expression"
                | "case_statement"
                | "match_conditional_expression"
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
        // PHP also spells them `and` / `or` (`xor` doesn't short-circuit, so it
        // adds no path).
        Language::Php => {
            if k == "binary_expression"
                && let Some(op) = node.child_by_field_name("operator")
            {
                let t = op.utf8_text(code.as_bytes()).unwrap_or("");
                return matches!(t, "&&" | "||" | "??" | "and" | "or");
            }
            false
        }
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

// ---- 3b. cognitive complexity ----
//
// Cyclomatic complexity counts paths; cognitive complexity estimates how hard
// the code is to *read*. Three rules (Campbell / SonarSource):
//   1. each break in the linear flow costs 1 (`if`, loop, `switch`, `catch`, …);
//   2. each of those nested inside another costs 1 *extra per level*, so deep
//      code is punished much harder than long flat code;
//   3. `else` / `else if` cost 1 flat — they add a branch, not a level — and a
//      *sequence* of the same boolean operator costs 1, not one per operator.
// A `switch` costs 1 whatever its number of cases: reading it is one decision.

pub fn cognitive(
    code: &str,
    tree: Option<&Tree>,
    lang: Language,
    override_max: usize,
) -> Vec<Issue> {
    let max = if override_max == 0 {
        default_max_cognitive(lang)
    } else {
        override_max
    };

    functions(tree, lang)
        .into_iter()
        .filter_map(|func| {
            let n = function_cognitive(func, lang, code);
            (n > max).then(|| {
                issue(
                    COGNITIVE_NAME,
                    format!("Complexité cognitive élevée : {n} (max {max}) — aplatis l'imbrication avec des clauses de garde"),
                    func,
                )
            })
        })
        .collect()
}

fn function_cognitive(func: Node, lang: Language, code: &str) -> usize {
    let mut score = 0usize;
    for child in named_children(func) {
        walk_cognitive(child, lang, code, 0, &mut score);
    }
    score
}

fn walk_cognitive(node: Node, lang: Language, code: &str, nesting: usize, score: &mut usize) {
    // A nested function is scored on its own, as with cyclomatic complexity.
    if is_function_kind(lang, node.kind()) {
        return;
    }

    let mut child_nesting = nesting;
    if is_cognitive_flow(lang, node.kind()) {
        if is_else_if(node) {
            // Already paid for as the `else` of the enclosing `if`, and it sits
            // at the same logical level — no increment, no extra nesting.
        } else {
            *score += 1 + nesting;
            child_nesting = nesting + 1;
        }
        // Each `else` / `elseif` branch is one flat point.
        *score += alternative_count(node);
    } else if is_logical_sequence_start(node, lang, code) {
        *score += 1;
    }

    for child in named_children(node) {
        walk_cognitive(child, lang, code, child_nesting, score);
    }
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Structures that break the linear flow *and* nest what they contain.
/// `switch`/`match` count once as a whole — their cases are not extra branches.
fn is_cognitive_flow(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "catch_clause"
                | "ternary_expression"
        ),
        Language::Python => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "while_statement"
                | "except_clause"
                | "conditional_expression"
                | "match_statement"
        ),
        Language::Java => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "enhanced_for_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "switch_expression"
                | "catch_clause"
                | "ternary_expression"
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
            "if_expression"
                | "while_expression"
                | "for_expression"
                | "loop_expression"
                | "match_expression"
        ),
        Language::Php => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "foreach_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "match_expression"
                | "catch_clause"
                | "conditional_expression"
        ),
        _ => false,
    }
}

/// How many `else` / `elseif` / `elif` branches hang off this node. Every
/// grammar here marks them with the `alternative` field, and PHP/Python can
/// carry several (`elseif … elseif … else`).
fn alternative_count(node: Node) -> usize {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return 0;
    }
    let mut n = 0;
    loop {
        if cursor.field_name() == Some("alternative") {
            n += 1;
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    n
}

/// The operator of a short-circuit boolean expression, if this node is one.
fn logical_operator<'a>(node: Node, lang: Language, code: &'a str) -> Option<&'a str> {
    let kind = node.kind();
    let is_logical_kind = match lang {
        Language::Python => kind == "boolean_operator",
        _ => kind == "binary_expression",
    };
    if !is_logical_kind {
        return None;
    }
    let op = node.child_by_field_name("operator")?;
    let text = op.utf8_text(code.as_bytes()).ok()?;
    matches!(text, "&&" | "||" | "and" | "or").then_some(text)
}

/// True at the *start* of a sequence of identical boolean operators: `a && b && c`
/// reads as one condition and costs 1, while `a && b || c` costs 2.
fn is_logical_sequence_start(node: Node, lang: Language, code: &str) -> bool {
    let Some(op) = logical_operator(node, lang, code) else {
        return false;
    };
    match node.parent() {
        Some(parent) => logical_operator(parent, lang, code) != Some(op),
        None => true,
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
                    format!("Imbrication trop profonde : {d} niveaux (max {max}) — utilise des retours anticipés ou extrais des fonctions"),
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
        // `else_if_clause` is deliberately absent: `elseif` is the same logical
        // level as its `if`, exactly like the `else if` case handled below.
        Language::Php => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "foreach_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "match_expression"
                | "try_statement"
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
                    format!("Classe fourre-tout : {n} méthodes (max {max}) — répartis les responsabilités dans des classes plus petites"),
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
        Language::Php => matches!(kind, "class_declaration" | "trait_declaration"),
        _ => false,
    }
}

fn is_method_kind(lang: Language, kind: &str) -> bool {
    match lang {
        Language::JavaScript | Language::TypeScript => kind == "method_definition",
        Language::Python => kind == "function_definition",
        Language::Java => matches!(kind, "method_declaration" | "constructor_declaration"),
        Language::Go | Language::Rust => false,
        Language::Php => kind == "method_declaration",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language::Php;

    fn parse(code: &str) -> Tree {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&Php.tree_sitter()).unwrap();
        p.parse(code, None).unwrap()
    }

    fn run(code: &str, f: fn(&str, Option<&Tree>, Language, usize) -> Vec<Issue>, max: usize) -> usize {
        let t = parse(code);
        f(code, Some(&t), Php, max).len()
    }

    /// Cyclomatic complexity of `function f() { <body> }` — the outer function,
    /// so a nested closure's own branches must not leak into it.
    fn complexity_of(body: &str) -> usize {
        let code = format!("<?php function f() {{ {body} }}");
        let t = parse(&code);
        let outer = functions(Some(&t), Php)
            .into_iter()
            .find(|f| f.kind() == "function_definition")
            .expect("outer function");
        function_complexity(outer, Php, &code)
    }

    #[test]
    fn finds_every_php_function_form() {
        let code = "<?php
class C {
    public function m() { return 1; }
}
function top() { return 2; }
$a = function () { return 3; };
$b = fn() => 4;
";
        let t = parse(code);
        assert_eq!(functions(Some(&t), Php).len(), 4);
    }

    #[test]
    fn counts_php_branches() {
        assert_eq!(complexity_of("return 1;"), 1); // base path only
        assert_eq!(complexity_of("if ($a) {} "), 2);
        // `elseif` is a branch; the trailing `else` is not.
        assert_eq!(complexity_of("if ($a) {} elseif ($b) {} else {}"), 3);
        // `else if` is an if nested in an else — same count, no double-count.
        assert_eq!(complexity_of("if ($a) {} else if ($b) {} else {}"), 3);
        assert_eq!(complexity_of("foreach ($xs as $x) {}"), 2);
        assert_eq!(complexity_of("while ($a) {}"), 2);
        assert_eq!(complexity_of("do {} while ($a);"), 2);
        assert_eq!(complexity_of("for ($i = 0; $i < 3; $i++) {}"), 2);
        assert_eq!(complexity_of("try {} catch (\\Throwable $e) {}"), 2);
        assert_eq!(complexity_of("$x = $a ? 1 : 2;"), 2);
        // `case` counts, `default` doesn't.
        assert_eq!(complexity_of("switch ($a) { case 1: break; default: break; }"), 2);
        assert_eq!(complexity_of("$x = match($a) { 1 => 'a', default => 'b' };"), 2);
        // Short-circuit operators, symbols and keywords alike; `xor` doesn't.
        assert_eq!(complexity_of("$x = $a && $b;"), 2);
        assert_eq!(complexity_of("$x = $a and $b;"), 2);
        assert_eq!(complexity_of("$x = $a ?? $b;"), 2);
        assert_eq!(complexity_of("$x = $a xor $b;"), 1);
    }

    #[test]
    fn nested_function_carries_its_own_complexity() {
        // The closure's `if` belongs to the closure, not to `f`.
        assert_eq!(complexity_of("$g = function ($x) { if ($x) {} }; return $g;"), 1);
    }

    #[test]
    fn counts_php_params_including_promotion() {
        let promoted = "<?php class C { public function __construct(private A $a, private B $b) {} }";
        assert_eq!(run(promoted, param_count, 1), 1);
        assert_eq!(run(promoted, param_count, 2), 0);
        let variadic = "<?php function f(int $a, string ...$rest) {}";
        assert_eq!(run(variadic, param_count, 1), 1);
    }

    #[test]
    fn measures_php_nesting_without_inflating_on_elseif() {
        let deep = "<?php function f() { if ($a) { foreach ($x as $y) { while ($z) { echo 1; } } } }";
        assert_eq!(run(deep, nesting, 2), 1);
        assert_eq!(run(deep, nesting, 3), 0);
        // if/elseif/else chains stay at one level.
        let chain = "<?php function f() { if ($a) {} elseif ($b) {} elseif ($c) {} else {} }";
        assert_eq!(run(chain, nesting, 1), 0);
    }

    #[test]
    fn counts_php_class_methods() {
        let cls = format!(
            "<?php class Big {{ {} }}",
            (0..5).map(|i| format!("public function m{i}() {{}}")).collect::<Vec<_>>().join(" ")
        );
        assert_eq!(run(&cls, god_class, 4), 1);
        assert_eq!(run(&cls, god_class, 5), 0);
        // An interface is a contract, not a god class.
        let iface = format!(
            "<?php interface Big {{ {} }}",
            (0..5).map(|i| format!("public function m{i}();")).collect::<Vec<_>>().join(" ")
        );
        assert_eq!(run(&iface, god_class, 4), 0);
    }

    /// Cognitive score of `function f() { <body> }`.
    fn cognitive_of(body: &str) -> usize {
        let code = format!("<?php function f() {{ {body} }}");
        let t = parse(&code);
        let outer = functions(Some(&t), Php)
            .into_iter()
            .find(|f| f.kind() == "function_definition")
            .expect("outer function");
        function_cognitive(outer, Php, &code)
    }

    #[test]
    fn cognitive_charges_for_nesting_not_just_branches() {
        assert_eq!(cognitive_of("return 1;"), 0); // linear code is free
        assert_eq!(cognitive_of("if ($a) {}"), 1);
        // Same three ifs: flat costs 3, nested costs 1 + 2 + 3.
        assert_eq!(cognitive_of("if ($a) {} if ($b) {} if ($c) {}"), 3);
        assert_eq!(cognitive_of("if ($a) { if ($b) { if ($c) {} } }"), 6);
        // A loop inside a loop, with a condition at the bottom.
        assert_eq!(
            cognitive_of("foreach ($a as $x) { foreach ($x as $y) { if ($y) {} } }"),
            1 + 2 + 3
        );
    }

    #[test]
    fn cognitive_counts_else_branches_flat() {
        assert_eq!(cognitive_of("if ($a) {} else {}"), 2); // if 1 + else 1
        assert_eq!(cognitive_of("if ($a) {} elseif ($b) {} else {}"), 3);
        assert_eq!(cognitive_of("if ($a) {} else if ($b) {} else {}"), 3);
        // The else body still nests: the inner if pays 1 + 1.
        assert_eq!(cognitive_of("if ($a) {} else { if ($b) {} }"), 4);
    }

    #[test]
    fn cognitive_counts_operator_sequences_once() {
        assert_eq!(cognitive_of("if ($a && $b && $c) {}"), 2); // if + one sequence
        assert_eq!(cognitive_of("if ($a && $b || $c) {}"), 3); // two sequences
        assert_eq!(cognitive_of("if ($a) {} $x = $b && $c;"), 2);
    }

    #[test]
    fn cognitive_charges_switch_once_and_skips_closures() {
        // One decision to read, whatever the number of cases.
        assert_eq!(
            cognitive_of("switch ($a) { case 1: break; case 2: break; case 3: break; default: break; }"),
            1
        );
        // The closure is scored on its own.
        assert_eq!(cognitive_of("$g = function ($x) { if ($x) {} }; return $g;"), 0);
    }

    #[test]
    fn cognitive_flags_over_the_threshold() {
        let deep = "<?php function f() { foreach ($a as $x) { foreach ($x as $y) { foreach ($y as $z) { if ($z) { if ($z > 1) {} } } } } }";
        // 1 + 2 + 3 + 4 + 5 = 15
        assert_eq!(run(deep, cognitive, 15), 0);
        assert_eq!(run(deep, cognitive, 14), 1);
    }

    #[test]
    fn measures_php_function_length() {
        let long = format!("<?php function f() {{\n{}\n}}", "$x = 1;\n".repeat(10));
        assert_eq!(run(&long, function_length, 5), 1);
        assert_eq!(run(&long, function_length, 50), 0);
    }
}
