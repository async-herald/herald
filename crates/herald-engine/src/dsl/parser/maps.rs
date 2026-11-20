//! Keyword → enum mappings for the rule header (category, severity, language).

use crate::category::Category;
use crate::language::Language;
use crate::types::Severity;

pub(super) fn parse_category(s: &str) -> Result<Category, String> {
    Ok(match s {
        "security" => Category::Security,
        "quality" | "code-smell" | "codesmell" => Category::Quality,
        "dependencies" => Category::Dependencies,
        "dead-code" | "deadcode" => Category::Quality,
        // Catégorie « Patterns IA » retirée du produit. L'alias reste accepté
        // pour que les règles personnalisées déjà écrites continuent de parser :
        // elles sont désormais rangées en qualité de code.
        "ai-pattern" | "ai" => Category::Quality,
        "git" | "versioning" => Category::Git,
        "pro-pattern" | "pattern" | "patterns" => Category::Pattern,
        "testing" | "test" | "tests" => Category::Testing,
        // Catégories « architecture », « performance » et « reliability »
        // retirées du produit (il n'en reste plus que 6 : sécurité, qualité,
        // versioning, pattern, dépendances, tests). Les alias survivent pour
        // que les règles déjà écrites (dont les .herald embarqués) continuent
        // de parser sans réécriture en masse.
        "architecture" => Category::Pattern,
        "performance" | "perf" => Category::Quality,
        "reliability" | "bug" | "bugs" => Category::Quality,
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
