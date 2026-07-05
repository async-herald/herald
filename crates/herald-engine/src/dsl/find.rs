//! Compile a `find` snippet into a tree-sitter query.
//!
//! Two forms:
//! - **code shape** — `find js, ts: exec($ARG)` : the snippet is parsed with the
//!   target grammar, then its AST is turned into a query. `$VAR` metavariables
//!   become wildcard captures (`(_) @VAR`); every concrete token becomes an
//!   `#eq?` constraint. The whole pattern is captured as `@match`.
//! - **regex** — `find: /(?i)md5/` : matches the regex against identifier and
//!   string nodes only (never comments / raw text), captured as `@match`.

use tree_sitter::{Node, Parser as TsParser, Query};

use super::model::Variant;
use crate::language::Language;

/// `$NAME` is rewritten to this identifier before parsing, then recovered as a
/// capture named `NAME`. Chosen to be a valid identifier in every grammar and
/// unlikely to collide with real source tokens.
const META_PREFIX: &str = "HERALDMETA_";

/// Compile a code-shape `find` into a [`Variant`] (one compiled query per lang).
///
/// `explicit` = the rule named the languages (so a compile failure is an author
/// error). When false (a bare `find:`), languages that don't parse the snippet
/// are silently dropped — only a total failure is an error.
pub fn compile_code(
    snippet: &str,
    languages: &[Language],
    explicit: bool,
    rule_id: &str,
) -> Result<Variant, String> {
    let rewritten = rewrite_metavars(snippet);

    let mut queries = Vec::new();
    let mut last_err = None;
    for &lang in languages {
        match compile_one(&rewritten, lang, rule_id) {
            Ok(q) => queries.push((lang, q)),
            Err(e) if explicit => return Err(e),
            Err(e) => last_err = Some(e),
        }
    }
    if queries.is_empty() {
        return Err(last_err.unwrap_or_else(|| format!("find produced no query in rule '{rule_id}'")));
    }

    let kept: Vec<Language> = queries.iter().map(|(l, _)| *l).collect();
    Ok(Variant {
        languages: kept,
        queries,
        predicate: None,
        report: "match".to_string(),
    })
}

fn compile_one(rewritten: &str, lang: Language, rule_id: &str) -> Result<Query, String> {
    let query_src = build_query(rewritten, lang, rule_id)?;
    let ts = lang.tree_sitter();
    Query::new(&ts, &query_src)
        .map_err(|e| format!("find error in rule '{rule_id}' ({lang:?}): {e}\n  query: {query_src}"))
}

/// Compile a regex `find` into a [`Variant`] matching identifier/string nodes.
pub fn compile_regex(regex: &str, languages: &[Language], rule_id: &str) -> Result<Variant, String> {
    let mut queries = Vec::new();
    let escaped = regex.replace('\\', "\\\\").replace('"', "\\\"");
    for &lang in languages {
        let alt = ident_string_kinds(lang)
            .iter()
            .map(|k| format!("({k})"))
            .collect::<Vec<_>>()
            .join(" ");
        let query_src = format!("([{alt}] @match (#match? @match \"{escaped}\"))");
        let ts = lang.tree_sitter();
        // Skip a grammar whose node kinds don't exist (best-effort coverage):
        // the regex rule still applies to every language that does compile.
        if let Ok(q) = Query::new(&ts, &query_src) {
            queries.push((lang, q));
        }
    }
    if queries.is_empty() {
        return Err(format!("regex find produced no query in rule '{rule_id}'"));
    }
    let kept: Vec<Language> = queries.iter().map(|(l, _)| *l).collect();
    Ok(Variant {
        languages: kept,
        queries,
        predicate: None,
        report: "match".to_string(),
    })
}

/// `$ARG` -> `HERALDMETA_ARG` so the snippet parses as ordinary code.
fn rewrite_metavars(snippet: &str) -> String {
    let bytes = snippet.as_bytes();
    let mut out = String::with_capacity(snippet.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$'
            && i + 1 < bytes.len()
            && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_')
        {
            out.push_str(META_PREFIX);
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                out.push(bytes[i] as char);
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Parse `code` with `lang`'s grammar and turn the meaningful root node into a
/// query string ending in `@match`.
fn build_query(snippet: &str, lang: Language, rule_id: &str) -> Result<String, String> {
    let mut parser = TsParser::new();
    parser
        .set_language(&lang.tree_sitter())
        .map_err(|e| format!("grammar load failed for {lang:?}: {e}"))?;

    // Plusieurs contextes d'enveloppe possibles (ex. C# : statement pour un
    // `throw`/appel, corps de lambda pour une comparaison). On prend le premier
    // qui parse le snippet sans erreur.
    let mut last_err =
        format!("find snippet '{snippet}' is not valid {lang:?} (rule '{rule_id}')");
    for (prefix, suffix) in wrap_candidates(lang) {
        let offset = prefix.len();
        let wrapped = format!("{prefix}{snippet}{suffix}");
        let Some(tree) = parser.parse(&wrapped, None) else {
            continue;
        };
        let end = offset + snippet.len();
        let Some(node) = tree.root_node().descendant_for_byte_range(offset, end) else {
            continue;
        };
        let root = climb_to_span(node, offset, end);
        if root.is_error() || root.kind() == "ERROR" || has_error(root) {
            continue;
        }
        let mut ctr = 0usize;
        let sexpr = compile_node(root, &wrapped, &mut ctr);
        if !sexpr.starts_with('(') {
            last_err = format!(
                "find snippet '{snippet}' is too unspecific in rule '{rule_id}' (would match everything)"
            );
            continue;
        }
        return Ok(format!("{sexpr} @match"));
    }
    Err(last_err)
}

/// Contextes d'enveloppe candidats rendant `snippet` valide dans `lang`. On en
/// essaie plusieurs (premier qui parse sans erreur) : certains langages exigent
/// des formes différentes selon que le snippet est un statement (appel, `throw`)
/// ou une expression nue (comparaison `x.Count() > 0`).
fn wrap_candidates(lang: Language) -> Vec<(&'static str, &'static str)> {
    match lang {
        // Langages où un appel nu est déjà valide au top-level.
        Language::JavaScript | Language::TypeScript | Language::Python
        | Language::Ruby | Language::Lua => vec![("", "")],
        Language::Java => vec![("class H{void m(){", ";}}")],
        // C# : statement d'abord (throw/appel), puis corps de lambda (comparaison).
        Language::CSharp => vec![
            ("class H{void m(){", ";}}"),
            ("class H{void m(){var _f=()=>", ";}}"),
        ],
        Language::Go => vec![("package p\nfunc m(){", "\n}")],
        Language::Rust => vec![("fn m(){", ";}")],
        Language::C | Language::Cpp | Language::Dart => vec![("void m(){", ";}")],
        Language::Swift => vec![("func m(){", "}")],
        Language::Kotlin => vec![("fun m(){", "}")],
        Language::Php => vec![("<?php ", ";")],
    }
}

/// Climb to the outermost ancestor that still spans exactly `[start, end)` —
/// `descendant_for_byte_range` can return an inner token when several nodes
/// share the span; we want the largest such node (the whole expression). Stop
/// before structural wrappers (program / expression_statement / block …) so the
/// reported `@match` is the real expression, not the whole file.
fn climb_to_span(node: Node, start: usize, end: usize) -> Node {
    const WRAPPERS: &[&str] = &[
        "program",
        "module",
        "source_file",
        "translation_unit",
        "expression_statement",
        "block",
        "statement_block",
        "statement",
    ];
    let mut cur = node;
    while let Some(parent) = cur.parent() {
        if parent.start_byte() >= start
            && parent.end_byte() <= end
            && !WRAPPERS.contains(&parent.kind())
        {
            cur = parent;
        } else {
            break;
        }
    }
    cur
}

fn has_error(node: Node) -> bool {
    node.has_error()
}

/// Recursively turn a node into a query S-expression. `$VAR` -> `(_) @VAR`;
/// concrete tokens -> `(kind) @_cN (#eq? @_cN "text")` (the `#eq?` is placed in
/// the enclosing parens so it is in scope).
fn compile_node(node: Node, code: &str, ctr: &mut usize) -> String {
    let text = node.utf8_text(code.as_bytes()).unwrap_or("");

    if let Some(name) = text.strip_prefix(META_PREFIX) {
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return format!("(_) @{name}");
        }
    }

    // True token (no children at all): pin its text with #eq?.
    if node.child_count() == 0 {
        let n = *ctr;
        *ctr += 1;
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("({}) @_c{n} (#eq? @_c{n} \"{}\")", node.kind(), escaped);
    }

    // Structural node: emit (kind field: child …) with each named child.
    let mut parts: Vec<String> = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.is_named() {
                let field = cursor.field_name();
                let cs = compile_node(child, code, ctr);
                match field {
                    Some(f) => parts.push(format!("{f}: {cs}")),
                    None => parts.push(cs),
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    if parts.is_empty() {
        format!("({})", node.kind())
    } else {
        format!("({} {})", node.kind(), parts.join(" "))
    }
}

/// Identifier- and string-bearing node kinds per grammar, for regex `find`.
fn ident_string_kinds(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::JavaScript | Language::TypeScript => {
            &["identifier", "property_identifier", "string", "template_string"]
        }
        Language::Python => &["identifier", "string"],
        Language::Go => &["identifier", "field_identifier", "interpreted_string_literal", "raw_string_literal"],
        Language::Java => &["identifier", "string_literal"],
        Language::Rust => &["identifier", "field_identifier", "string_literal"],
        Language::Php => &["name", "string", "encapsed_string"],
        Language::CSharp => &["identifier", "string_literal"],
        Language::Ruby => &["identifier", "constant", "string"],
        Language::Kotlin => &["identifier", "string_literal"],
        Language::C => &["identifier", "string_literal"],
        Language::Cpp => &["identifier", "string_literal"],
        Language::Swift => &["simple_identifier", "line_string_literal"],
        Language::Lua => &["identifier", "string"],
        Language::Dart => &["identifier", "string_literal"],
    }
}
