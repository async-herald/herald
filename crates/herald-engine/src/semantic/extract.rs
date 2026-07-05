use std::collections::HashSet;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor, Tree};

use crate::language::Language;

use super::lang;

#[derive(Debug, Default)]
pub struct FileSymbols {
    pub import_specs: Vec<(String, usize)>,
    pub named_imports: HashSet<String>,
    pub star_specs: Vec<String>,
    pub exports: Vec<(String, usize)>,
    pub is_barrel: bool,
}

pub fn extract(tree: Option<&Tree>, code: &str, language: Language) -> FileSymbols {
    let mut sym = FileSymbols::default();
    let (Some(tree), Some(def)) = (tree, lang::def(language)) else {
        return sym;
    };
    let ts = language.tree_sitter();

    for (cap, text, line) in query_all(tree, &ts, code, def.query) {
        match cap.as_str() {
            "spec" => sym.import_specs.push((unquote(&text), line)),
            "named" => {
                sym.named_imports.insert(text);
            }
            "star" => sym.star_specs.push(unquote(&text)),
            "export" => sym.exports.push((text, line)),
            _ => {}
        }
    }

    if let Some(marker) = def.barrel_marker {
        sym.is_barrel = code.contains(marker);
    }
    sym
}

fn query_all(
    tree: &Tree,
    ts: &tree_sitter::Language,
    code: &str,
    src: &str,
) -> Vec<(String, String, usize)> {
    let Ok(query) = Query::new(ts, src) else {
        return Vec::new();
    };
    let names = query.capture_names();
    let b = code.as_bytes();

    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(&query, tree.root_node(), b);

    let mut out = Vec::new();
    while let Some(m) = it.next() {
        for cap in m.captures {
            let name = names.get(cap.index as usize).copied().unwrap_or("").to_string();
            let text = cap.node.utf8_text(b).unwrap_or("").to_string();
            out.push((name, text, cap.node.start_position().row + 1));
        }
    }
    out
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string()
}
