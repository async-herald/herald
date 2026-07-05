mod blocks;
mod maps;
mod predicate;
mod scanner;

use super::model::{Dedupe, Engine, Example, Pred, RuleDef, Variant};
use crate::language::Language;
use maps::{parse_category, parse_severity};
use scanner::Parser;

pub fn parse_rule(src: &str) -> Result<RuleDef, String> {
    let mut p = Parser::new(src);
    p.ws();
    match p.ident()?.as_str() {
        "rule" => {}
        other => return Err(format!("expected 'rule', got '{other}'")),
    }
    let id = p.ident()?;
    p.ws();
    // `[cat/sev]` is optional — it may instead live in a `meta { … }` block.
    let mut category: Option<crate::category::Category> = None;
    let mut severity: Option<crate::types::Severity> = None;
    if p.peek() == Some('[') {
        p.expect('[')?;
        category = Some(parse_category(&p.ident()?)?);
        p.ws();
        p.expect('/')?;
        severity = Some(parse_severity(&p.ident()?)?);
        p.ws();
        p.expect(']')?;
    }
    p.ws();
    p.expect('{')?;

    let mut message = String::new();
    let mut dedupe = Dedupe::None;
    let mut shared: Option<Pred> = None;
    let mut variants: Vec<Variant> = Vec::new();
    let mut metric_key: Option<String> = None;
    let mut analyzer_key: Option<String> = None;
    let mut th_default: Option<usize> = None;
    let mut th_per_lang: Vec<(Language, usize)> = Vec::new();
    let mut examples: Vec<Example> = Vec::new();
    let mut context: Option<Engine> = None;

    loop {
        p.ws();
        if p.eat('}') {
            break;
        }
        let kw = p.ident()?;
        match kw.as_str() {
            "message" => message = p.string()?,
            "dedupe" => {
                dedupe = match p.ident()?.as_str() {
                    "line" => Dedupe::Line,
                    "linecol" => Dedupe::LineCol,
                    "none" => Dedupe::None,
                    other => return Err(format!("unknown dedupe '{other}'")),
                }
            }
            // legacy precise form
            "where" => shared = Some(p.predicate()?),
            "lang" => variants.push(p.lang_block(&id)?),
            "metric" => metric_key = Some(p.ident()?),
            "builtin" => analyzer_key = Some(p.ident()?),
            "threshold" => p.threshold(&mut th_default, &mut th_per_lang)?,
            // meta block: type / severity / message
            "meta" => p.meta_block(&mut category, &mut severity, &mut message)?,
            // match: either a block `match { <lang>: forme … }` or one line `match <langs>: forme`
            "match" | "find" => {
                p.ws();
                if p.peek() == Some('{') {
                    variants.extend(p.match_block(&id)?);
                } else {
                    variants.push(p.find_block(&id)?);
                }
            }
            "when" => shared = Some(p.predicate()?),
            "measure" => {
                let (key, default, per_lang) = p.measure()?;
                metric_key = Some(key);
                th_default = Some(default);
                th_per_lang = per_lang;
            }
            "use" => analyzer_key = Some(p.ident()?),
            // context rules: match the file path / git branch / commit message
            "path" => context = Some(p.context_rule(super::model::Target::Path)?),
            "branch" => context = Some(p.context_rule(super::model::Target::Branch)?),
            "commit" => context = Some(p.context_rule(super::model::Target::Commit)?),
            // tests block (new) / examples block (legacy) — same semantics
            "tests" => examples = p.tests_block()?,
            "examples" => examples = p.examples_block()?,
            other => return Err(format!("unexpected directive '{other}' in rule '{id}'")),
        }
    }

    let engine = match context {
        Some(ctx) => ctx,
        None => build_engine(&id, metric_key, analyzer_key, th_default, th_per_lang, shared, variants, &message)?,
    };

    let category = category.ok_or_else(|| format!("rule '{id}' has no category ([cat/sev] or meta.type)"))?;
    let severity = severity.ok_or_else(|| format!("rule '{id}' has no severity ([cat/sev] or meta.severity)"))?;

    Ok(RuleDef {
        id,
        category,
        severity,
        message,
        dedupe,
        engine,
        examples,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_engine(
    id: &str,
    metric_key: Option<String>,
    analyzer_key: Option<String>,
    th_default: Option<usize>,
    th_per_lang: Vec<(Language, usize)>,
    shared: Option<Pred>,
    variants: Vec<Variant>,
    message: &str,
) -> Result<Engine, String> {
    if let Some(key) = metric_key {
        return Ok(Engine::Metric {
            key,
            default: th_default,
            per_lang: th_per_lang,
        });
    }
    if let Some(key) = analyzer_key {
        return Ok(Engine::Analyzer { key });
    }
    if variants.is_empty() {
        return Err(format!("rule '{id}' has no lang block / metric / analyzer"));
    }
    if message.is_empty() {
        return Err(format!("query rule '{id}' has no message"));
    }
    Ok(Engine::Query { shared, variants })
}
