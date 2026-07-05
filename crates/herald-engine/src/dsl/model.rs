//! Runtime model for data-driven rules (parsed from the `.herald` DSL).

use regex::Regex;
use tree_sitter::{Node, Query};

use crate::category::Category;
use crate::language::Language;
use crate::types::Severity;

pub struct RuleDef {
    pub id: String,
    pub category: Category,
    pub severity: Severity,
    pub message: String,
    pub dedupe: Dedupe,
    pub engine: Engine,
    /// Inline test cases from the `examples { … }` block, run by `herald test`.
    pub examples: Vec<Example>,
}

/// What a test case (or `path`/`branch`/`commit` rule) is evaluated against.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Source code in a given language (AST/regex rules).
    Code(Language),
    /// A file path (`path:` rules).
    Path,
    /// The current git branch name (`branch:` rules).
    Branch,
    /// The current commit message (`commit:` rules).
    Commit,
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Code(l) => format!("{l:?}"),
            Target::Path => "path".into(),
            Target::Branch => "branch".into(),
            Target::Commit => "commit".into(),
        }
    }
}

/// One inline test case. `flag … => fail`, `ok … => pass`. `value` is the code
/// snippet (Code target) or the path/branch/commit string (context targets).
pub struct Example {
    pub should_flag: bool,
    pub target: Target,
    pub value: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dedupe {
    None,
    Line,
    LineCol,
}

pub enum Engine {
    /// tree-sitter query per language + optional predicate on captured text.
    Query {
        shared: Option<Pred>, // applied to every language (merged AND)
        variants: Vec<Variant>,
    },
    /// A built-in metric (counting) mechanism routed by key, with thresholds.
    Metric {
        key: String,
        default: Option<usize>,
        per_lang: Vec<(Language, usize)>,
    },
    /// A built-in analyzer (traversal / regex / freq-map) routed by key.
    /// The mechanism stays in Rust; the declaration lives here.
    Analyzer { key: String },
    /// A match on the analysis *context* rather than code: a file path
    /// (`path:`), the git branch (`branch:`), or the commit message (`commit:`).
    /// `negated` flips it (flag when the value does NOT match — e.g. a branch
    /// that doesn't follow the naming convention).
    Context {
        field: Target,
        regex: Regex,
        negated: bool,
    },
}

impl Engine {
    /// Threshold for a language (per-lang override, else default, else 0 = auto).
    pub fn threshold_for(default: &Option<usize>, per_lang: &[(Language, usize)], lang: Option<Language>) -> usize {
        lang.and_then(|l| per_lang.iter().find(|(pl, _)| *pl == l).map(|(_, v)| *v))
            .or(*default)
            .unwrap_or(0)
    }
}

pub struct Variant {
    pub languages: Vec<Language>,
    /// Compiled query, one per language in `languages` (same source, different grammar).
    pub queries: Vec<(Language, Query)>,
    pub predicate: Option<Pred>,
    /// Capture name to report the position of (default "match").
    pub report: String,
}

/// Numeric comparison for function predicates (`entropy(@v) > 4`).
#[derive(Clone, Copy)]
pub enum Cmp {
    Gt,
    Ge,
    Lt,
    Le,
}

/// Named structural conditions: `$x is dynamic` / `is constant` / `is user-input`.
#[derive(Clone, Copy)]
pub enum IsKind {
    /// Built from runtime parts: concatenation, template/interpolation, format/Sprintf.
    Dynamic,
    /// A fixed literal value (string/number/bool with no interpolation) — not a
    /// variable, call, member access, or env lookup. Use `not $x is constant`
    /// for "any non-literal argument".
    Constant,
    /// Reads request/CLI input (req.query, request.args, argv, getParameter…).
    UserInput,
}

/// Predicate over the named captures of a match.
pub enum Pred {
    All(Vec<Pred>),
    Any(Vec<Pred>),
    Not(Box<Pred>),
    /// `@<capture> ~ /regex/` (or `!~` when `negated`).
    Regex {
        capture: String,
        regex: Regex,
        negated: bool,
    },
    /// A ready-made function on a capture: `placeholder(@v)` (boolean) or
    /// `entropy(@v) > 4` / `len(@v) > 100` / `lines(@v) > 50` (numeric).
    Func {
        name: String,
        capture: String,
        cmp: Option<(Cmp, f64)>,
    },
    /// `$<capture> is <kind>` — a structural condition over the matched node.
    Is { capture: String, kind: IsKind },
}

/// Capture name -> matched node. Borrows from the parsed tree; node text and
/// kind are derived on demand against the source. No per-match allocation.
pub type Captures<'a> = std::collections::HashMap<&'a str, Node<'a>>;

impl Pred {
    pub fn eval(&self, caps: &Captures, code: &str, lang: Language) -> bool {
        let text_of = |cap: &str| {
            caps.get(cap)
                .map(|n| n.utf8_text(code.as_bytes()).unwrap_or(""))
                .unwrap_or("")
        };
        match self {
            Pred::All(ps) => ps.iter().all(|p| p.eval(caps, code, lang)),
            Pred::Any(ps) => ps.iter().any(|p| p.eval(caps, code, lang)),
            Pred::Not(p) => !p.eval(caps, code, lang),
            Pred::Regex {
                capture,
                regex,
                negated,
            } => regex.is_match(text_of(capture)) != *negated,
            Pred::Func { name, capture, cmp } => eval_func(name, text_of(capture), *cmp),
            Pred::Is { capture, kind } => caps
                .get(capture.as_str())
                .is_some_and(|n| is_cond(*kind, *n, code, lang)),
        }
    }
}

// ---- `is …` structural conditions ----

fn is_cond(kind: IsKind, node: Node, code: &str, lang: Language) -> bool {
    match kind {
        IsKind::Dynamic => is_dynamic(node, code, lang),
        IsKind::Constant => is_literal(node),
        IsKind::UserInput => {
            let text = node.utf8_text(code.as_bytes()).unwrap_or("");
            USER_INPUT.is_match(text)
        }
    }
}

/// True if the node is a fixed literal value (string/number/bool) with no
/// interpolation — i.e. a constant the developer typed, not a variable or call.
fn is_literal(node: Node) -> bool {
    match node.kind() {
        "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal" => {
            !has_descendant_kind(node, "interpolation")
        }
        "template_string" | "template_literal" => !has_descendant_kind(node, "template_substitution"),
        "number" | "integer" | "float" | "integer_literal" | "float_literal" => true,
        "true" | "false" | "null" | "none" | "boolean" | "nil" | "None" | "True" | "False" => true,
        _ => false,
    }
}

/// True if the node is built from runtime parts rather than a single literal:
/// string concatenation, template/f-string interpolation, or a `format`/`Sprintf`
/// style call. Works across grammars by inspecting node kinds and descendants.
fn is_dynamic(node: Node, code: &str, _lang: Language) -> bool {
    match node.kind() {
        // concatenation / arithmetic: `a + b`, `"x" % y`, `s += x`
        "binary_expression" | "binary_operator" | "augmented_assignment" => true,
        // JS/TS template literal `` `...${x}...` `` — dynamic only with a substitution
        "template_string" | "template_literal" => {
            has_descendant_kind(node, "template_substitution")
        }
        // Python f-string: a `string` node containing an `interpolation`
        "string" | "string_literal" => has_descendant_kind(node, "interpolation"),
        // format-style calls: Sprintf / String.format / "...".format(...) / format!()
        "call_expression" | "call" | "method_invocation" | "macro_invocation" => {
            let text = node.utf8_text(code.as_bytes()).unwrap_or("");
            FORMAT_CALL.is_match(text)
        }
        _ => false,
    }
}

fn has_descendant_kind(node: Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    let mut stack: Vec<Node> = node.children(&mut cursor).collect();
    while let Some(n) = stack.pop() {
        if n.kind() == kind {
            return true;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    false
}

static USER_INPUT: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?x)
        req(uest)?\.(query|params|body|cookies|headers|args|form|values|GET|POST)
        | \.getParameter\b | \.getHeader\b
        | request\.(GET|POST|args|form|values|cookies|headers|json)
        | os\.environ | process\.argv | sys\.argv | \bargv\b
        | r\.(URL|Form|PostForm)\b | \.FormValue\b | \.Query\(\)
        ",
    )
    .unwrap()
});

static FORMAT_CALL: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?x) \bSprintf\b | \bFprintf\b | \.format\b | \bformat!\s*\( | \bString\.format\b")
        .unwrap()
});

// ---- ready-made `where` functions ----

static PLACEHOLDER: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?i)^(?:\s*|x{3,}|\*{3,}|\.{3,}|changeme|placeholder|example|sample|test|todo|none|null|undefined|your[_-].*|<.*>|\{\{.*\}\}|\$\{.*\}|process\.env.*|os\.environ.*)$"#).unwrap()
});

static FIRST_STRING: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#"["'`]([^"'`]*)["'`]"#).unwrap());

fn eval_func(name: &str, text: &str, cmp: Option<(Cmp, f64)>) -> bool {
    match name {
        // boolean: the (first quoted) value looks like a placeholder / env ref
        "placeholder" => {
            let value = FIRST_STRING
                .captures(text)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or(text);
            PLACEHOLDER.is_match(value)
        }
        // numeric: compared against `cmp`
        "entropy" => compare(shannon_entropy(strip_quotes(text)), cmp),
        "len" => compare(strip_quotes(text).chars().count() as f64, cmp),
        "lines" => compare(text.lines().count() as f64, cmp),
        _ => false,
    }
}

fn compare(value: f64, cmp: Option<(Cmp, f64)>) -> bool {
    match cmp {
        Some((Cmp::Gt, n)) => value > n,
        Some((Cmp::Ge, n)) => value >= n,
        Some((Cmp::Lt, n)) => value < n,
        Some((Cmp::Le, n)) => value <= n,
        None => false, // numeric function used without a comparison
    }
}

fn strip_quotes(text: &str) -> &str {
    text.trim()
        .trim_start_matches(['"', '\'', '`'])
        .trim_end_matches(['"', '\'', '`'])
}

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let len = s.chars().count() as f64;
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    -counts
        .values()
        .map(|&n| {
            let p = n as f64 / len;
            p * p.log2()
        })
        .sum::<f64>()
}
