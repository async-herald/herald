//! Keyword → enum mappings for the rule header (category, severity, language).

use crate::category::Category;
use crate::language::Language;
use crate::types::Severity;

pub(super) fn parse_category(s: &str) -> Result<Category, String> {
    Ok(match s {
        "security" => Category::Security,
        "code-smell" | "codesmell" => Category::CodeSmell,
        "dependencies" => Category::Dependencies,
        "architecture" => Category::Architecture,
        "dead-code" | "deadcode" => Category::CodeSmell,
        "ai-pattern" | "ai" => Category::Ai,
        "git" => Category::Git,
        "performance" | "perf" => Category::Performance,
        "pro-pattern" | "pattern" | "patterns" => Category::Pattern,
        "reliability" | "bug" | "bugs" => Category::Reliability,
        "testing" | "test" | "tests" => Category::Testing,
        other => return Err(format!("unknown category '{other}'")),
    })
}

pub(super) fn parse_severity(s: &str) -> Result<Severity, String> {
    Ok(match s {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" => Severity::Info,
        other => return Err(format!("unknown severity '{other}'")),
    })
}

pub(super) fn parse_language(s: &str) -> Result<Language, String> {
    // Tous les langages enregistrés (extensions + alias) sont acceptés.
    Language::from_name(s).ok_or_else(|| format!("unknown language '{s}'"))
}
