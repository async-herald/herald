
mod eval;
mod find;
pub mod model;
mod parser;

use std::sync::LazyLock;

use tree_sitter::Tree;

use crate::language::Language;
use crate::types::Issue;

use model::{Engine, RuleDef};

/// Embedded built-in rule sources. Each is one `.herald` file.
use include_dir::{include_dir, Dir};

/// Every `.herald` rule file, embedded at compile time from `rules/`.
static RULES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/rules");

/// All embedded rule sources, discovered recursively and sorted by path for a
/// deterministic order. Adding a `.herald` file under `rules/` is enough — there
/// is no list to maintain here.
fn builtin_sources() -> Vec<&'static str> {
    fn walk(dir: &'static Dir<'static>, out: &mut Vec<(&'static str, &'static str)>) {
        for f in dir.files() {
            if f.path().extension().and_then(|e| e.to_str()) == Some("herald")
                && let Some(s) = f.contents_utf8()
            {
                out.push((f.path().to_str().unwrap_or(""), s));
            }
        }
        for d in dir.dirs() {
            walk(d, out);
        }
    }
    let mut v = Vec::new();
    walk(&RULES_DIR, &mut v);
    v.sort_by_key(|(p, _)| *p);
    v.into_iter().map(|(_, s)| s).collect()
}

/// When a rule applies to a given file — precomputed once so `run_all` can skip
/// irrelevant rules without entering the interpreter.
enum Applies {
    /// File-level metric (line-length, file-lines): runs even without a language.
    Always,
    /// Analyzer or language metric: runs for any recognized language.
    AnyLang,
    /// Query rule: runs only for the languages it has a variant for.
    Langs(Vec<Language>),
    /// Context rule (path/branch/commit): not run per-file by `run_all` — it is
    /// evaluated against the analysis context by `run_context`.
    Context,
}

fn applicability(rule: &RuleDef) -> Applies {
    match &rule.engine {
        Engine::Query { variants, .. } => {
            let mut langs = Vec::new();
            for v in variants {
                for &l in &v.languages {
                    if !langs.contains(&l) {
                        langs.push(l);
                    }
                }
            }
            Applies::Langs(langs)
        }
        Engine::Metric { key, .. } if key == "file_lines" || key == "line_length" => Applies::Always,
        Engine::Metric { .. } | Engine::Analyzer { .. } => Applies::AnyLang,
        Engine::Context { .. } => Applies::Context,
    }
}

struct Loaded {
    rule: RuleDef,
    applies: Applies,
}

static RULES: LazyLock<Vec<Loaded>> = LazyLock::new(|| {
    builtin_sources()
        .into_iter()
        .filter_map(|src| match parser::parse_rule(src) {
            Ok(rule) => {
                let applies = applicability(&rule);
                Some(Loaded { rule, applies })
            }
            Err(e) => {
                eprintln!("herald: DSL rule parse error: {e}");
                None
            }
        })
        .collect()
});

/// Run every enabled, applicable DSL rule against a file (BUILTIN order). Rules
/// whose language doesn't match are skipped before entering the interpreter.
pub fn run_all(
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
    enabled: impl Fn(&str) -> bool,
) -> Vec<Issue> {
    run_loaded(&RULES, code, tree, language, enabled)
}

fn run_loaded(
    rules: &[Loaded],
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
    enabled: impl Fn(&str) -> bool,
) -> Vec<Issue> {
    let mut out = Vec::new();
    for loaded in rules {
        let applies = match &loaded.applies {
            Applies::Always => true,
            Applies::AnyLang => language.is_some(),
            Applies::Langs(ls) => language.is_some_and(|l| ls.contains(&l)),
            Applies::Context => false, // handled by run_context, not per-file
        };
        if applies && enabled(&loaded.rule.id) {
            out.extend(eval::run(&loaded.rule, code, tree, language));
        }
    }
    out
}

/// Jeu de règles compilé depuis des sources `.herald` arbitraires (ex. la base),
/// à exécuter à la place du catalogue compilé dans le binaire.
pub struct CompiledRules(Vec<Loaded>);

impl CompiledRules {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Compile des sources `.herald` (règles erronées ignorées avec un log).
pub fn compile(sources: &[String]) -> CompiledRules {
    CompiledRules(
        sources
            .iter()
            .filter_map(|src| match parser::parse_rule(src) {
                Ok(rule) => {
                    let applies = applicability(&rule);
                    Some(Loaded { rule, applies })
                }
                Err(e) => {
                    eprintln!("herald: DSL rule parse error: {e}");
                    None
                }
            })
            .collect(),
    )
}

/// Comme `run_all`, mais sur un jeu de règles compilé fourni (base de données).
pub fn run_all_on(
    set: &CompiledRules,
    code: &str,
    tree: Option<&Tree>,
    language: Option<Language>,
    enabled: impl Fn(&str) -> bool,
) -> Vec<Issue> {
    run_loaded(&set.0, code, tree, language, enabled)
}

/// Compile a single rule from its `.herald` source and run it against one code
/// snippet — for "test this rule on this code" (rule editor / `analyze snippet`).
/// Returns only that rule's findings, or a parse error.
pub fn run_source(source: &str, code: &str, language: Language) -> Result<Vec<Issue>, String> {
    let rule = parser::parse_rule(source)?;
    // Context rules (path/branch/commit) test the value directly, not as code.
    if matches!(rule.engine, Engine::Context { .. }) {
        return Ok(eval::run_context(&rule, code));
    }
    let tree = crate::parser::parse(code, language);
    Ok(eval::run(&rule, code, tree.as_ref(), Some(language)))
}

/// Métadonnées d'une règle (id, catégorie, sévérité, message) extraites de sa
/// source `.herald`. Valide la source d'une règle personnalisée et dénormalise
/// ses champs sans dupliquer le parseur ailleurs. `Err(msg)` si la source ne
/// parse pas — le message est celui du parseur.
pub struct RuleMeta {
    pub id: String,
    pub category: String,
    pub severity: String,
    pub message: String,
}

pub fn parse_meta(source: &str) -> Result<RuleMeta, String> {
    let rule = parser::parse_rule(source)?;
    let severity = match rule.severity {
        crate::types::Severity::Error => "error",
        crate::types::Severity::Warning => "warning",
        crate::types::Severity::Info => "info",
    };
    Ok(RuleMeta {
        id: rule.id,
        category: rule.category.name().to_string(),
        severity: severity.to_string(),
        message: rule.message,
    })
}

/// Raw `.herald` source of a built-in (official) rule, by id.
pub fn builtin_source(id: &str) -> Option<&'static str> {
    builtin_rules().into_iter().find(|r| r.id == id).map(|r| r.source)
}

/// Run all context rules (`path:`/`branch:`/`commit:`) against the analysis
/// context. `path` is per-file (call once per file); `branch`/`commit` are
/// repo-global (call once). A `None` field skips the rules targeting it.
pub fn run_context(
    path: Option<&str>,
    branch: Option<&str>,
    commit: Option<&str>,
    enabled: impl Fn(&str) -> bool,
) -> Vec<Issue> {
    let mut out = Vec::new();
    for loaded in RULES.iter() {
        let Engine::Context { field, .. } = &loaded.rule.engine else {
            continue;
        };
        if !enabled(&loaded.rule.id) {
            continue;
        }
        let value = match field {
            model::Target::Path => path,
            model::Target::Branch => branch,
            model::Target::Commit => commit,
            model::Target::Code(_) => None,
        };
        if let Some(v) = value {
            out.extend(eval::run_context(&loaded.rule, v));
        }
    }
    out
}

/// Outcome of one inline `examples` test case.
pub struct ExampleOutcome {
    pub rule: String,
    pub target: String,
    pub value: String,
    pub expected_flag: bool,
    pub got_flag: bool,
}

impl ExampleOutcome {
    pub fn passed(&self) -> bool {
        self.expected_flag == self.got_flag
    }
}

/// Run every rule's `examples`/`tests` block and check that `flag`/`fail` cases
/// are detected and `ok`/`pass` cases are not. Powers `herald test`.
pub fn run_examples() -> Vec<ExampleOutcome> {
    RULES
        .iter()
        .flat_map(|loaded| {
            let rule = &loaded.rule;
            rule.examples.iter().map(move |ex| example_outcome(rule, ex))
        })
        .collect()
}

/// Run the `tests`/`examples` block of one rule given its `.herald` source —
/// the "check" of the desktop rule editor. `Err` is the parser message.
pub fn run_source_examples(source: &str) -> Result<Vec<ExampleOutcome>, String> {
    let rule = parser::parse_rule(source)?;
    Ok(rule
        .examples
        .iter()
        .map(|ex| example_outcome(&rule, ex))
        .collect())
}

fn example_outcome(rule: &model::RuleDef, ex: &model::Example) -> ExampleOutcome {
    let got_flag = match ex.target {
        model::Target::Code(lang) => {
            let tree = crate::parser::parse(&ex.value, lang);
            let issues = eval::run(rule, &ex.value, tree.as_ref(), Some(lang));
            issues.iter().any(|i| i.rule == rule.id)
        }
        model::Target::Path | model::Target::Branch | model::Target::Commit => {
            !eval::run_context(rule, &ex.value).is_empty()
        }
    };
    ExampleOutcome {
        rule: rule.id.clone(),
        target: ex.target.label(),
        value: ex.value.clone(),
        expected_flag: ex.should_flag,
        got_flag,
    }
}

/// (id, category) for every loaded DSL rule — for the `rules` listing.
pub fn rule_list() -> Vec<(String, String)> {
    RULES
        .iter()
        .map(|l| (l.rule.id.clone(), l.rule.category.name().to_string()))
        .collect()
}

/// A built-in rule with its raw `.herald` source — used to seed the DB.
pub struct BuiltinRule {
    pub id: String,
    pub category: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub source: &'static str,
}

/// Every embedded rule with metadata + raw source, so the host can seed them
/// into the database as official (`owner IS NULL`) rules.
pub fn builtin_rules() -> Vec<BuiltinRule> {
    use crate::types::Severity;
    builtin_sources()
        .into_iter()
        .filter_map(|src| {
            parser::parse_rule(src).ok().map(|r| BuiltinRule {
                id: r.id,
                category: r.category.name(),
                severity: match r.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "info",
                },
                message: r.message,
                source: src,
            })
        })
        .collect()
}

#[cfg(test)]
mod source_example_tests {
    use super::run_source_examples;

    const RULE: &str = r#"
rule demo-eval {
  meta {
    type: security
    severity: error
    message: "eval sur une entrée"
  }
  match js: eval($X)
  when $X is user-input
  tests {
    js {
      "eval(req.query.expr)" => fail
      "eval('1 + 1')" => pass
    }
  }
}
"#;

    #[test]
    fn the_tests_of_one_rule_run_from_its_source() {
        let outcomes = run_source_examples(RULE).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|o| o.passed()));
        assert!(outcomes[0].expected_flag && outcomes[0].got_flag);
    }

    #[test]
    fn a_wrong_expectation_is_reported() {
        let wrong = RULE.replace("\"eval('1 + 1')\" => pass", "\"eval('1 + 1')\" => fail");
        let outcomes = run_source_examples(&wrong).unwrap();
        assert!(!outcomes[1].passed());
    }

    #[test]
    fn an_unreadable_source_gives_the_parser_message() {
        assert!(run_source_examples("rule {").is_err());
    }
}
