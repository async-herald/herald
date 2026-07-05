use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "unused-parameter",
    category: Category::CodeSmell,
    check,
};

fn is_function_kind(language: Language, kind: &str) -> bool {
    match language {
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

fn ident_name<'a>(node: Node<'a>, code: &str) -> Option<(String, Node<'a>)> {
    (node.kind() == "identifier")
        .then(|| (node.utf8_text(code.as_bytes()).unwrap_or("").to_string(), node))
}

// Extract a simple parameter name from a param node; None for destructuring /
// rest / variadic / anything we can't cleanly attribute.
fn simple_param<'a>(node: Node<'a>, code: &str) -> Option<(String, Node<'a>)> {
    match node.kind() {
        "identifier" => ident_name(node, code),
        // TS typed params, Python default/typed params expose a name/pattern.
        "required_parameter" | "optional_parameter" => node
            .child_by_field_name("pattern")
            .and_then(|p| ident_name(p, code)),
        "default_parameter" | "typed_parameter" | "typed_default_parameter" => node
            .child_by_field_name("name")
            .and_then(|p| ident_name(p, code))
            .or_else(|| {
                let mut c = node.walk();
                node.named_children(&mut c)
                    .find(|n| n.kind() == "identifier")
                    .and_then(|n| ident_name(n, code))
            }),
        "assignment_pattern" => node
            .child_by_field_name("left")
            .and_then(|p| ident_name(p, code)),
        _ => None,
    }
}

fn body_of<'a>(func: Node<'a>) -> Option<Node<'a>> {
    func.child_by_field_name("body")
}

fn count_idents(node: Node, lang: Language, code: &str, map: &mut HashMap<String, usize>) {
    let k = node.kind();
    if k == "identifier"
        || (matches!(lang, Language::JavaScript | Language::TypeScript)
            && k == "shorthand_property_identifier")
    {
        let t = node.utf8_text(code.as_bytes()).unwrap_or("");
        *map.entry(t.to_string()).or_default() += 1;
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        count_idents(child, lang, code, map);
    }
}

fn skip_name(name: &str) -> bool {
    name.starts_with('_') || matches!(name, "self" | "cls" | "this")
}

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    if !matches!(
        language,
        Language::JavaScript | Language::TypeScript | Language::Python
    ) {
        return Vec::new();
    }
    let Some(tree) = tree else {
        return Vec::new();
    };

    let mut funcs = Vec::new();
    collect_functions(tree.root_node(), language, &mut funcs);

    let mut issues = Vec::new();
    for func in funcs {
        let Some(params) = func.child_by_field_name("parameters") else {
            continue;
        };
        let Some(body) = body_of(func) else {
            continue; // no body (signature/abstract) — can't judge usage
        };

        let mut used_in_body = HashMap::new();
        count_idents(body, language, code, &mut used_in_body);

        // (optional name, optional decl node, is_used) per positional param.
        let mut slots: Vec<(Option<(String, Node)>, bool)> = Vec::new();
        let mut c = params.walk();
        for child in params.named_children(&mut c) {
            if matches!(child.kind(), "comment") {
                continue;
            }
            match simple_param(child, code) {
                Some((name, node)) => {
                    let used = skip_name(&name)
                        || used_in_body.get(&name).copied().unwrap_or(0) > 0;
                    slots.push((Some((name, node)), used));
                }
                // Destructuring/rest/variadic: treat as "used" so it doesn't get
                // flagged and so earlier params before it stay eligible.
                None => slots.push((None, true)),
            }
        }

        // after-used: only flag unused params before the last used one.
        let Some(last_used) = slots.iter().rposition(|(_, used)| *used) else {
            continue;
        };
        for (i, (info, used)) in slots.iter().enumerate() {
            if *used || i >= last_used {
                continue;
            }
            if let Some((name, node)) = info {
                let p = node.start_position();
                issues.push(Issue {
                    rule: META.name.to_string(),
                    category: Category::CodeSmell,
                    severity: Severity::Warning,
                    message: format!(
                        "Unused parameter '{name}' — remove it or prefix with '_'"
                    ),
                    line: p.row + 1,
                    column: p.column + 1,
                });
            }
        }
    }

    issues.sort_by_key(|i| (i.line, i.column));
    issues.dedup_by_key(|i| (i.line, i.column));
    issues
}
