use std::collections::HashSet;

use crate::language::Language;

pub struct LangDef {
    pub lang: Language,
    pub query: &'static str,
    pub barrel_marker: Option<&'static str>,
    pub resolve: fn(&str, &str, &HashSet<&str>) -> Option<String>,
}

pub fn def(language: Language) -> Option<&'static LangDef> {
    DEFS.iter().find(|d| d.lang == language)
}

pub fn resolve(
    from_file: &str,
    language: Language,
    spec: &str,
    files: &HashSet<&str>,
) -> Option<String> {
    def(language).and_then(|d| (d.resolve)(from_file, spec, files))
}

const DEFS: &[LangDef] = &[
    LangDef { lang: Language::JavaScript, query: JS, barrel_marker: Some("export *"), resolve: resolve_js },
    LangDef { lang: Language::TypeScript, query: JS, barrel_marker: Some("export *"), resolve: resolve_js },
    LangDef { lang: Language::Python, query: PY, barrel_marker: None, resolve: resolve_python },
    LangDef { lang: Language::Rust, query: RS, barrel_marker: None, resolve: resolve_rust },
    LangDef { lang: Language::Go, query: GO, barrel_marker: None, resolve: resolve_none },
    LangDef { lang: Language::Java, query: JAVA, barrel_marker: None, resolve: resolve_none },
];

const JS: &str = r#"
(import_statement source: (string) @spec)
(export_statement source: (string) @spec)
(import_specifier name: (identifier) @named)
(export_statement (export_clause (export_specifier name: (identifier) @named)) source: (string))
(import_statement (import_clause (namespace_import)) source: (string) @star)
(export_statement declaration: (function_declaration name: (identifier) @export))
(export_statement declaration: (class_declaration name: (_) @export))
(export_statement declaration: (lexical_declaration (variable_declarator name: (identifier) @export)))
(export_statement (export_clause (export_specifier name: (identifier) @export)))
"#;

const PY: &str = "(import_from_statement module_name: (relative_import) @spec)";

const RS: &str = "(mod_item name: (identifier) @spec)";

const GO: &str = "(import_spec path: (interpreted_string_literal) @spec)";

const JAVA: &str = "(import_declaration (scoped_identifier) @spec)";

fn dir_of(path: &str) -> Vec<String> {
    let mut parts: Vec<String> = path.split('/').map(String::from).collect();
    parts.pop();
    parts
}

fn join(mut base: Vec<String>, rel: &str) -> String {
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                base.pop();
            }
            s => base.push(s.to_string()),
        }
    }
    base.join("/")
}

fn resolve_js(from_file: &str, spec: &str, files: &HashSet<&str>) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let target = join(dir_of(from_file), spec);
    if files.contains(target.as_str()) {
        return Some(target);
    }
    const EXTS: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx"];
    for e in EXTS {
        let c = format!("{target}.{e}");
        if files.contains(c.as_str()) {
            return Some(c);
        }
    }
    for e in EXTS {
        let c = format!("{target}/index.{e}");
        if files.contains(c.as_str()) {
            return Some(c);
        }
    }
    None
}

fn resolve_python(from_file: &str, spec: &str, files: &HashSet<&str>) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let dots = spec.chars().take_while(|c| *c == '.').count();
    let rest = &spec[dots..];
    let mut base = dir_of(from_file);
    for _ in 1..dots {
        base.pop();
    }
    let target = join(base, &rest.replace('.', "/"));
    let c1 = format!("{target}.py");
    if files.contains(c1.as_str()) {
        return Some(c1);
    }
    let c2 = format!("{target}/__init__.py");
    if files.contains(c2.as_str()) {
        return Some(c2);
    }
    None
}

fn resolve_rust(from_file: &str, spec: &str, files: &HashSet<&str>) -> Option<String> {
    let target = join(dir_of(from_file), spec);
    let c1 = format!("{target}.rs");
    if files.contains(c1.as_str()) {
        return Some(c1);
    }
    let c2 = format!("{target}/mod.rs");
    if files.contains(c2.as_str()) {
        return Some(c2);
    }
    None
}

fn resolve_none(_from_file: &str, _spec: &str, _files: &HashSet<&str>) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Query;

    #[test]
    fn all_queries_compile() {
        for d in DEFS {
            let ts = d.lang.tree_sitter();
            if let Err(e) = Query::new(&ts, d.query) {
                panic!("{:?}: {e:?}", d.lang);
            }
        }
    }
}
