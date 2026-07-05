use std::sync::LazyLock;

use regex::Regex;
use tree_sitter::Tree;

use crate::category::Category;
use crate::helpers::ast_query;
use crate::language::Language;
use crate::types::{FnRule, Issue, Severity};

pub const META: FnRule = FnRule {
    name: "nosql_injection",
    category: Category::Security,
    check,
};

const MSG: &str = "Possible NoSQL injection: untrusted input reaches a query filter or a $where/$function operator — validate and cast input, never pass req.body/query objects straight into the filter";

const WHERE_JS: &str = r#"(pair key: [(string) (property_identifier)] @k (#match? @k "[$]where|[$]function|[$]accumulator") value: [(template_string (template_substitution)) (binary_expression)]) @match"#;
const WHERE_PY: &str = r#"(pair key: (string) @k (#match? @k "[$]where|[$]function|[$]accumulator") value: [(string (interpolation)) (binary_operator)]) @match"#;
const WHERE_JAVA: &str = r#"
(object_creation_expression
  type: (type_identifier) @t
  arguments: (argument_list
    (string_literal) @k
    (binary_expression))
  (#match? @t "^(Document|BasicDBObject)$")
  (#match? @k "[$]where|[$]function|[$]accumulator")) @match

(object_creation_expression
  type: (type_identifier) @t
  arguments: (argument_list
    (string_literal) @k
    (method_invocation name: (identifier) @fmt))
  (#match? @t "^(Document|BasicDBObject)$")
  (#match? @k "[$]where|[$]function|[$]accumulator")
  (#eq? @fmt "format")) @match

(method_invocation
  object: (identifier) @o
  name: (identifier) @w
  arguments: (argument_list (binary_expression))
  (#eq? @o "Filters")
  (#eq? @w "where")) @match
"#;
const METHOD_JS: &str = r#"(call_expression function: (member_expression property: (property_identifier) @m) (#match? @m "^(find|findOne|findOneAndUpdate|findOneAndDelete|findOneAndReplace|updateOne|updateMany|update|replaceOne|deleteOne|deleteMany|delete|remove|count|countDocuments|distinct|aggregate)$") arguments: (arguments . (_) @match))"#;
const METHOD_PY: &str = r#"(call function: (attribute attribute: (identifier) @m) (#match? @m "^(find|find_one|find_one_and_update|find_one_and_delete|find_one_and_replace|update_one|update_many|update|delete_one|delete_many|delete|remove|count_documents|distinct|aggregate)$") arguments: (argument_list . (_) @match))"#;

static RE_JS: LazyLock<(Regex, Regex)> = LazyLock::new(|| (
    Regex::new(r"\b(?:req|request|ctx|context)\.(?:body|query|params)\b").unwrap(),
    Regex::new(r"(?:ObjectId|Number|parseInt|parseFloat|String|Boolean|mongoSanitize|sanitize|escapeRegExp)\s*\(\s*(?:req|request|ctx|context)\.(?:body|query|params)").unwrap(),
));
static RE_PY: LazyLock<(Regex, Regex)> = LazyLock::new(|| (
    Regex::new(r"\brequest\.(?:args|form|json|data|values|GET|POST)").unwrap(),
    Regex::new(r"(?:ObjectId|int|float|str|bool|escape|sanitize)\s*\(\s*request\.(?:args|form|json|data|values|GET|POST)").unwrap(),
));

pub fn check(code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
    let flag = |line, column| Issue {
        rule: META.name.to_string(),
        category: Category::Security,
        severity: Severity::Error,
        message: MSG.to_string(),
        line,
        column,
    };

    let mut issues = Vec::new();

    let where_q = match language {
        Language::JavaScript | Language::TypeScript => Some(WHERE_JS),
        Language::Python => Some(WHERE_PY),
        Language::Java => Some(WHERE_JAVA),
        _ => None,
    };
    if let Some(q) = where_q {
        issues.extend(ast_query(tree, code, language, q, |_, l, c| vec![flag(l, c)]));
    }

    let method = match language {
        Language::JavaScript | Language::TypeScript => Some((METHOD_JS, &RE_JS.0, &RE_JS.1)),
        Language::Python => Some((METHOD_PY, &RE_PY.0, &RE_PY.1)),
        _ => None,
    };
    if let Some((q, input, cast)) = method {
        issues.extend(ast_query(tree, code, language, q, |t, l, c| {
            let n = input.find_iter(t).count();
            if n > 0 && n > cast.find_iter(t).count() {
                vec![flag(l, c)]
            } else {
                Vec::new()
            }
        }));
    }

    issues.sort_by_key(|i| (i.line, i.column));
    issues.dedup_by_key(|i| (i.line, i.column));
    issues
}