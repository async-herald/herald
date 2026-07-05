use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "xxe",
    category: Category::Security,
    check,
};

const MSG: &str = "Possible XXE: an XML parser is created without disabling DTDs / external entities — disable doctype and external entity resolution (or use a hardened/defused parser)";

// Java: instantiation of an XML parser factory…
static JAVA_PARSER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:DocumentBuilderFactory|SAXParserFactory|XMLInputFactory|TransformerFactory|SchemaFactory|XMLReaderFactory)\s*\.\s*newInstance|new\s+(?:SAXReader|SAXBuilder)\s*\(").unwrap()
});
// …unless the file hardens it somewhere.
static JAVA_HARDENED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)disallow-doctype-decl|FEATURE_SECURE_PROCESSING|external-general-entities|external-parameter-entities|ACCESS_EXTERNAL_(?:DTD|SCHEMA)|setExpandEntityReferences\s*\(\s*false|XMLConstants").unwrap()
});

// Python: lxml/etree configured to resolve entities.
static PY_INSECURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"resolve_entities\s*=\s*True|no_network\s*=\s*False|load_dtd\s*=\s*True").unwrap()
});

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let flag = |line, column| Issue {
        rule: META.name.to_string(),
        category: Category::Security,
        severity: Severity::Error,
        message: MSG.to_string(),
        line,
        column,
    };

    let mut issues = match language {
        Language::Java => {
            if JAVA_HARDENED.is_match(code) {
                Vec::new()
            } else {
                ast_query(tree, code, language, "(method_invocation) @match", |t, l, c| {
                    if JAVA_PARSER.is_match(t) {
                        vec![flag(l, c)]
                    } else {
                        Vec::new()
                    }
                })
                .into_iter()
                .chain(ast_query(
                    tree,
                    code,
                    language,
                    "(object_creation_expression) @match",
                    |t, l, c| {
                        if JAVA_PARSER.is_match(t) {
                            vec![flag(l, c)]
                        } else {
                            Vec::new()
                        }
                    },
                ))
                .collect()
            }
        }
        Language::Python => ast_query(tree, code, language, "(call) @match", |t, l, c| {
            if PY_INSECURE.is_match(t) {
                vec![flag(l, c)]
            } else {
                Vec::new()
            }
        }),
        _ => Vec::new(),
    };

    issues.sort_by_key(|i| (i.line, i.column));
    issues.dedup_by_key(|i| i.line);
    issues
}
