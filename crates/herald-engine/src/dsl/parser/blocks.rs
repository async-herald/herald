//! Block-level parsing: `lang … { match … where … }`, raw query extraction,
//! and the `threshold` directive.

use tree_sitter::Query;

use super::maps::{parse_category, parse_language, parse_severity};
use super::scanner::Parser;
use crate::category::Category;
use crate::dsl::find;
use crate::dsl::model::{Example, Target, Variant};
use crate::language::Language;
use crate::types::Severity;

/// Languages a bare `find:` (no language list) is attempted against.
const ALL_LANGS: &[Language] = &[
    Language::JavaScript,
    Language::TypeScript,
    Language::Python,
    Language::Go,
    Language::Java,
    Language::Rust,
    Language::Php,
    Language::CSharp,
    Language::Ruby,
    Language::Kotlin,
    Language::C,
    Language::Cpp,
    Language::Swift,
    Language::Lua,
    Language::Dart,
];

impl Parser<'_> {
    /// `lang <l1>, <l2> { pattern <query>  [where <pred>] }` → compiled [`Variant`].
    pub(super) fn lang_block(&mut self, rule_id: &str) -> Result<Variant, String> {
        let mut languages = Vec::new();
        loop {
            languages.push(parse_language(&self.ident()?)?);
            self.ws();
            if !self.eat(',') {
                break;
            }
        }
        self.ws();
        self.expect('{')?;

        self.ws();
        match self.ident()?.as_str() {
            "pattern" => {}
            other => return Err(format!("expected 'pattern' in rule '{rule_id}', got '{other}'")),
        }
        let query_src = self.query_text();
        if query_src.trim().is_empty() {
            return Err(format!("empty query in rule '{rule_id}'"));
        }

        let mut predicate = None;
        let save = self.pos;
        if self.try_keyword("where") {
            predicate = Some(self.predicate()?);
        } else {
            self.pos = save;
        }

        self.ws();
        self.expect('}')?;

        // Compile the query for each language of this variant (same source,
        // different grammar).
        let mut queries = Vec::new();
        for &lang in &languages {
            let ts = lang.tree_sitter();
            let q = Query::new(&ts, query_src.trim())
                .map_err(|e| format!("query error in rule '{rule_id}' ({lang:?}): {e}"))?;
            queries.push((lang, q));
        }

        Ok(Variant {
            languages,
            queries,
            predicate,
            report: "match".to_string(),
        })
    }

    /// `meta { type: … severity: … message: "…" }` → fills category/severity/message.
    pub(super) fn meta_block(
        &mut self,
        category: &mut Option<Category>,
        severity: &mut Option<Severity>,
        message: &mut String,
    ) -> Result<(), String> {
        self.ws();
        self.expect('{')?;
        loop {
            self.ws();
            if self.eat('}') {
                break;
            }
            let key = self.ident()?;
            self.ws();
            self.expect(':')?;
            match key.as_str() {
                "type" | "category" => *category = Some(parse_category(&self.ident()?)?),
                "severity" => *severity = Some(parse_severity(&self.ident()?)?),
                "message" => *message = self.string()?,
                other => return Err(format!("unknown meta field '{other}'")),
            }
        }
        Ok(())
    }

    /// `match { <langs>: forme   <langs>: forme … }` → one [`Variant`] per line.
    pub(super) fn match_block(&mut self, rule_id: &str) -> Result<Vec<Variant>, String> {
        self.ws();
        self.expect('{')?;
        let mut variants = Vec::new();
        loop {
            self.ws();
            if self.eat('}') {
                break;
            }
            variants.push(self.find_block(rule_id)?);
        }
        Ok(variants)
    }

    /// `tests { <lang> { "<code>" => fail | pass … } … }` → [`Example`]s.
    /// `fail` = must be detected, `pass` = must not. Code is double-quoted (use
    /// single quotes inside) so `=>` and braces in the code don't terminate it.
    pub(super) fn tests_block(&mut self) -> Result<Vec<Example>, String> {
        self.ws();
        self.expect('{')?;
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.eat('}') {
                break;
            }
            // group header: a language (`js {`) or a context target (`path {`)
            let tok = self.ident()?;
            let target = match tok.as_str() {
                "path" => Target::Path,
                "branch" => Target::Branch,
                "commit" => Target::Commit,
                other => Target::Code(parse_language(other)?),
            };
            self.ws();
            self.expect('{')?;
            loop {
                self.ws();
                if self.eat('}') {
                    break;
                }
                let value = self.string()?;
                self.ws();
                if !self.rest().starts_with("=>") {
                    return Err(format!("expected '=>' after test value at byte {}", self.pos));
                }
                self.pos += 2;
                let should_flag = match self.ident()?.as_str() {
                    "fail" => true,
                    "pass" => false,
                    other => return Err(format!("expected 'fail' or 'pass', got '{other}'")),
                };
                out.push(Example {
                    should_flag,
                    target,
                    value,
                });
            }
        }
        Ok(out)
    }

    /// `path|branch|commit [not] : /regex/` → a context-matching engine.
    pub(super) fn context_rule(&mut self, field: Target) -> Result<crate::dsl::model::Engine, String> {
        self.ws();
        let negated = self.try_keyword("not");
        self.ws();
        self.expect(':')?;
        self.ws();
        let pattern = self.regex_literal()?;
        let regex = regex::Regex::new(&pattern).map_err(|e| format!("bad regex /{pattern}/: {e}"))?;
        Ok(crate::dsl::model::Engine::Context { field, regex, negated })
    }

    /// Raw query text after `match`, up to a top-level `where` or `}`. Tracks
    /// string literals and ()/[] depth so those tokens inside the query don't
    /// terminate it (queries never contain `{`/`}`).
    fn query_text(&mut self) -> String {
        self.ws();
        let start = self.pos;
        let mut depth = 0i32;
        let mut in_str = false;
        while let Some(c) = self.peek() {
            if in_str {
                self.pos += c.len_utf8();
                if c == '"' {
                    in_str = false;
                } else if c == '\\'
                    && let Some(n) = self.peek() {
                        self.pos += n.len_utf8();
                    }
                continue;
            }
            match c {
                '"' => {
                    in_str = true;
                    self.pos += 1;
                }
                '(' | '[' => {
                    depth += 1;
                    self.pos += 1;
                }
                ')' | ']' => {
                    depth -= 1;
                    self.pos += 1;
                }
                '}' if depth <= 0 => break,
                _ if depth <= 0 && self.rest().starts_with("where") => {
                    // only terminate on the `where` keyword (word boundary)
                    let after = self.s[self.pos + 5..].chars().next();
                    if after.is_none_or(|a| !a.is_ascii_alphanumeric() && a != '_') {
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                _ => self.pos += c.len_utf8(),
            }
        }
        self.s[start..self.pos].to_string()
    }

    /// `find [<l1>, <l2>]: <snippet>` → compiled [`Variant`].
    ///
    /// With no language list the snippet is attempted against every grammar and
    /// the ones that don't parse it are dropped. A `/…/` snippet is a regex
    /// `find`; anything else is a code-shape `find`.
    pub(super) fn find_block(&mut self, rule_id: &str) -> Result<Variant, String> {
        self.ws();
        // Optional language list; absent when `find:` / `find {` come directly.
        let (langs, explicit): (Vec<Language>, bool) =
            if matches!(self.peek(), Some(':') | Some('{')) {
                (ALL_LANGS.to_vec(), false)
            } else {
                let mut langs = Vec::new();
                loop {
                    langs.push(parse_language(&self.ident()?)?);
                    self.ws();
                    if !self.eat(',') {
                        break;
                    }
                    self.ws();
                }
                (langs, true)
            };

        // Body: either `{ code }` (braces, balanced) or `: code` (rest of line).
        self.ws();
        let snippet = if self.eat('{') {
            self.brace_body()?
        } else if self.eat(':') {
            self.rest_of_line()
        } else {
            return Err(format!("expected '{{' or ':' after find in rule '{rule_id}'"));
        };
        if snippet.is_empty() {
            return Err(format!("empty find in rule '{rule_id}'"));
        }
        if snippet.len() >= 2 && snippet.starts_with('/') && snippet.ends_with('/') {
            let re = &snippet[1..snippet.len() - 1];
            find::compile_regex(re, &langs, rule_id)
        } else {
            find::compile_code(&snippet, &langs, explicit, rule_id)
        }
    }

    /// `examples { flag <lang>: <code>  ok <lang>: <code> … }`.
    pub(super) fn examples_block(&mut self) -> Result<Vec<Example>, String> {
        self.ws();
        self.expect('{')?;
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.eat('}') {
                break;
            }
            let should_flag = match self.ident()?.as_str() {
                "flag" => true,
                "ok" => false,
                other => return Err(format!("expected 'flag' or 'ok' in examples, got '{other}'")),
            };
            let language = parse_language(&self.ident()?)?;
            self.ws();
            self.expect(':')?;
            let value = self.rest_of_line();
            if value.is_empty() {
                return Err("empty example code".into());
            }
            out.push(Example {
                should_flag,
                target: Target::Code(language),
                value,
            });
        }
        Ok(out)
    }

    /// `measure <key> [> N] [(<lang> N, …)]` → (key, default, per-language).
    pub(super) fn measure(&mut self) -> Result<(String, usize, Vec<(Language, usize)>), String> {
        let key = self.ident()?;
        self.ws();
        // optional comparison operator (cosmetic — metrics always flag "≥ N")
        if matches!(self.peek(), Some('>') | Some('<') | Some('=')) {
            self.bump();
            self.eat('=');
        }
        let default = self.number()?;
        let mut per_lang = Vec::new();
        self.ws();
        if self.eat('(') {
            loop {
                let lang = parse_language(&self.ident()?)?;
                let n = self.number()?;
                per_lang.push((lang, n));
                self.ws();
                if !self.eat(',') {
                    break;
                }
            }
            self.ws();
            self.expect(')')?;
        }
        Ok((key, default, per_lang))
    }

    /// `threshold default=50, java=70, go=60` → default + per-language map.
    pub(super) fn threshold(
        &mut self,
        default: &mut Option<usize>,
        per_lang: &mut Vec<(Language, usize)>,
    ) -> Result<(), String> {
        loop {
            let key = self.ident()?;
            self.ws();
            self.expect('=')?;
            let n = self.number()?;
            if key == "default" {
                *default = Some(n);
            } else {
                per_lang.push((parse_language(&key)?, n));
            }
            self.ws();
            if !self.eat(',') {
                break;
            }
        }
        Ok(())
    }
}
