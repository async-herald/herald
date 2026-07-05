//! Execute a parsed `RuleDef` against a file's syntax tree.

use std::collections::HashMap;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor, Tree};

use crate::builtins::{architecture, codesmell, security};
use crate::language::Language;
use crate::types::Issue;

use super::model::{Dedupe, Engine, RuleDef, Variant};

pub fn run(rule: &RuleDef, code: &str, tree: Option<&Tree>, language: Option<Language>) -> Vec<Issue> {
    let mut issues = match &rule.engine {
        Engine::Query { shared, variants } => {
            let (Some(tree), Some(language)) = (tree, language) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for variant in variants {
                if variant.languages.contains(&language) {
                    run_variant(rule, shared.as_ref(), variant, code, tree, language, &mut out);
                }
            }
            out
        }
        Engine::Metric {
            key,
            default,
            per_lang,
        } => {
            let max = Engine::threshold_for(default, per_lang, language);
            run_metric(key, max, code, tree, language)
        }
        Engine::Analyzer { key } => {
            let Some(language) = language else {
                return Vec::new();
            };
            run_analyzer(key, code, tree, language)
        }
        // Context rules (path/branch/commit) aren't code-based — see `run_context`.
        Engine::Context { .. } => Vec::new(),
    };

    // La catégorie déclarée dans le `.herald` (meta.type) fait foi : on l'impose
    // à chaque relevé, même quand le mécanisme builtin en code-ait une autre.
    // (La sévérité par-relevé, elle, reste celle émise par le mécanisme.)
    for issue in &mut issues {
        issue.category = rule.category.clone();
    }

    match rule.dedupe {
        Dedupe::Line => {
            issues.sort_by_key(|i| (i.line, i.column));
            issues.dedup_by_key(|i| i.line);
        }
        Dedupe::LineCol => {
            issues.sort_by_key(|i| (i.line, i.column));
            issues.dedup_by_key(|i| (i.line, i.column));
        }
        Dedupe::None => {}
    }
    issues
}

/// Evaluate a context rule (`path:`/`branch:`/`commit:`) against `value` (a file
/// path, a branch name, or a commit message). Returns one issue if it matches
/// (or doesn't, when `negated`). Non-context rules return nothing.
pub fn run_context(rule: &RuleDef, value: &str) -> Vec<Issue> {
    let Engine::Context { regex, negated, .. } = &rule.engine else {
        return Vec::new();
    };
    if regex.is_match(value) != *negated {
        vec![Issue {
            rule: rule.id.clone(),
            category: rule.category.clone(),
            severity: rule.severity.clone(),
            message: rule.message.clone(),
            line: 1,
            column: 1,
        }]
    } else {
        Vec::new()
    }
}

/// Built-in counting metrics, routed by key. Threshold comes from the rule.
fn run_metric(
    key: &str,
    max: usize,
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
) -> Vec<Issue> {
    use codesmell::{long_line, max_file_lines, metrics};
    let lang_metric = |f: fn(&str, Option<&Tree>, Language, usize) -> Vec<Issue>| {
        language.map(|l| f(code, tree, l, max)).unwrap_or_default()
    };
    match key {
        "function_lines" => lang_metric(metrics::function_length),
        "params" => lang_metric(metrics::param_count),
        "complexity" => lang_metric(metrics::complexity),
        "nesting" => lang_metric(metrics::nesting),
        "class_methods" => lang_metric(metrics::god_class),
        "file_lines" => max_file_lines::check(code, max),
        "line_length" => long_line::check(code, max),
        _ => Vec::new(),
    }
}

/// Built-in analyzers (traversal / freq-map / custom), routed by key.
fn run_analyzer(key: &str, code: &str, tree: Option<&Tree>, lang: Language) -> Vec<Issue> {
    match key {
        // code-smell
        "todo_fixme" => codesmell::todo_fixme::check(code, tree, lang),
        "no_unused_vars" => codesmell::no_unused_vars::check(code, tree, lang),
        "unused_import" => codesmell::unused_import::check(code, tree, lang),
        "commented_out_code" => codesmell::commented_out_code::check(code, tree, lang),
        "naming_conventions" => codesmell::naming_conventions::check(code, tree, lang),
        "unreachable_code" => codesmell::unreachable_code::check(code, tree, lang),
        "empty_catch" => codesmell::empty_catch::check(code, tree, lang),
        "unused_parameter" => codesmell::unused_parameter::check(code, tree, lang),
        // architecture
        "high_coupling" => architecture::high_coupling::check(code, tree, lang),
        "deep_relative_import" => architecture::deep_relative_import::check(code, tree, lang),
        // security
        "nosql_injection" => security::nosql_injection::check(code, tree, lang),
        "xxe" => security::xxe::check(code, tree, lang),
        "taint_command_injection" => security::taint::check_command_injection(code, tree, lang),
        "taint_code_injection" => security::taint::check_code_injection(code, tree, lang),
        "taint_xss" => security::taint::check_xss(code, tree, lang),
        "taint_sql_injection" => security::taint::check_sql_injection(code, tree, lang),
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_variant(
    rule: &RuleDef,
    shared: Option<&super::model::Pred>,
    variant: &Variant,
    code: &str,
    tree: &Tree,
    language: Language,
    out: &mut Vec<Issue>,
) {
    let Some((_, query)) = variant.queries.iter().find(|(l, _)| *l == language) else {
        return;
    };
    let names = query.capture_names();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), code.as_bytes());

    while let Some(m) = matches.next() {
        // Borrows: keys from the query's capture names, nodes from the tree.
        let mut caps: HashMap<&str, Node> = HashMap::new();
        for cap in m.captures {
            let name = names[cap.index as usize];
            caps.entry(name).or_insert(cap.node);
        }

        if let Some(p) = shared
            && !p.eval(&caps, code, language) {
                continue;
            }
        if let Some(p) = &variant.predicate
            && !p.eval(&caps, code, language) {
                continue;
            }

        let (line, column) = caps
            .get(variant.report.as_str())
            .or_else(|| caps.values().next())
            .map(|n| {
                let p = n.start_position();
                (p.row + 1, p.column + 1)
            })
            .unwrap_or((1, 1));

        out.push(Issue {
            rule: rule.id.clone(),
            category: rule.category.clone(),
            severity: rule.severity.clone(),
            message: rule.message.clone(),
            line,
            column,
        });
    }
}
