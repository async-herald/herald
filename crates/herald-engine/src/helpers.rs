use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor, Tree};

use crate::category::Category;
use crate::language::Language;
use crate::types::{Issue, Rule, Severity};

pub struct AstRule {
    pub name: &'static str,
    pub category: Category,
    pub severity: Severity,
    pub message: &'static str,
    pub queries: &'static [(Language, &'static str)],
}

impl Rule for AstRule {
    fn name(&self) -> &'static str {
        self.name
    }
    fn category(&self) -> &Category {
        &self.category
    }
    fn run(&self, code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
        scan_ast(tree, code, language, self)
    }
}

thread_local! {
    static QUERY_CACHE: RefCell<HashMap<(usize, Language), Query>> = RefCell::new(HashMap::new());
}

fn with_query<R>(
    language: Language,
    source: &'static str,
    f: impl FnOnce(&Query) -> R,
) -> Option<R> {
    let key = (source.as_ptr() as usize, language);
    QUERY_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let query = match cache.entry(key) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let ts = language.tree_sitter();
                let query = Query::new(&ts, source).ok()?;
                e.insert(query)
            }
        };
        Some(f(query))
    })
}

pub fn ast_query<F>(
    tree: Option<&Tree>,
    code: &str,
    language: Language,
    query_source: &'static str,
    mut visit: F,
) -> Vec<Issue>
where
    F: FnMut(&str, usize, usize) -> Vec<Issue>,
{
    let Some(tree) = tree else {
        return Vec::new();
    };

    with_query(language, query_source, |query| {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), code.as_bytes());

        let mut issues = Vec::new();
        while let Some(m) = matches.next() {
            if let Some(capture) = m.captures.first() {
                let node = capture.node;
                let text = node.utf8_text(code.as_bytes()).unwrap_or("");
                let start = node.start_position();
                issues.extend(visit(text, start.row + 1, start.column + 1));
            }
        }
        issues
    })
    .unwrap_or_default()
}

pub fn scan_ast(tree: Option<&Tree>, code: &str, language: Language, rule: &AstRule) -> Vec<Issue> {
    let Some(tree) = tree else {
        return Vec::new();
    };
    let Some(&(_, query_source)) = rule.queries.iter().find(|(l, _)| *l == language) else {
        return Vec::new();
    };

    with_query(language, query_source, |query| {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), code.as_bytes());

        let mut issues = Vec::new();
        while let Some(m) = matches.next() {
            if let Some(capture) = m.captures.first() {
                let start = capture.node.start_position();
                issues.push(Issue {
                    rule: rule.name.to_string(),
                    category: rule.category.clone(),
                    severity: rule.severity.clone(),
                    message: rule.message.to_string(),
                    line: start.row + 1,
                    column: start.column + 1,
                });
            }
        }
        issues
    })
    .unwrap_or_default()
}
