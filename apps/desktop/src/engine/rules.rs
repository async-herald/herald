//! Règles d'analyse : catalogue en ligne s'il y en a un (`crate::service`),
//! sinon catalogue intégré au moteur. Les règles maintenues par l'app
//! (`rules/`, voir `local_rules`) remplacent celles de même identifiant.

use gpui::SharedString;
use herald_engine::dsl::BuiltinRule;
use serde::Deserialize;

use super::local_rules;
use crate::model::{Category, Rule, RuleCatalog, RuleOrigin, Severity};

#[derive(Deserialize)]
struct RulesResponse {
    rules: Vec<RemoteRule>,
}

/// Règle du catalogue en ligne.
#[derive(Deserialize)]
struct RemoteRule {
    id: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    message: Option<String>,
    source: String,
    enabled: bool,
}

impl RemoteRule {
    fn into_rule(self) -> Option<Rule> {
        let source = effective_source(&self.id, self.source);
        Some(Rule {
            id: self.id.into(),
            category: Category::from_engine(self.category.as_deref()?)?,
            severity: Severity::from_engine(self.severity.as_deref().unwrap_or_default()),
            message: self.message.unwrap_or_default().into(),
            source,
            enabled: self.enabled,
            default_enabled: self.enabled,
        })
    }
}

/// Catalogue en ligne ; liste vide sans service, hors ligne ou en cas d'erreur.
fn fetch_official() -> Vec<RemoteRule> {
    crate::service::official_rules_json()
        .and_then(|body| serde_json::from_str::<RulesResponse>(&body).ok())
        .map(|body| body.rules)
        .unwrap_or_default()
}

/// Sources des règles actives et leur origine (`true` : herald.codes).
///
/// Règles officielles, choix de l'utilisateur compris (une règle désactivée sur
/// herald.codes peut être réactivée) ; hors ligne, catalogue intégré au moteur.
/// Dans les deux cas, les règles maintenues par l'app (`rules/`) remplacent
/// celles de même identifiant.
pub(super) fn sources() -> (Vec<String>, bool) {
    let overrides = crate::store::rule_overrides();
    let official: Vec<String> = fetch_official()
        .into_iter()
        .filter(|rule| overrides.get(&rule.id).copied().unwrap_or(rule.enabled))
        .filter(|rule| has_dsl_rule(&rule.source))
        .map(|rule| rule.source)
        .collect();
    if official.is_empty() {
        (local_rules::builtin_sources(), false)
    } else {
        (local_rules::merge(official), true)
    }
}

/// Faux pour une règle écrite en Rust dans le moteur : le catalogue n'en donne
/// qu'un commentaire (`// Règle Rust — pas de fichier .herald …`), que le
/// parseur DSL rejetterait à chaque analyse. Le moteur l'exécute de toute façon.
fn has_dsl_rule(source: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with("//")
    })
}

/// Source affichée sur l'écran « Règles » : celle que l'analyse exécute.
fn effective_source(id: &str, source: String) -> SharedString {
    local_rules::sources()
        .get(id)
        .map_or(source, |local| (*local).to_owned())
        .into()
}

/// Règles désactivées par l'utilisateur, ajoutées à `rules.disable` : le moteur
/// les ignore toutes, y compris celles écrites en Rust et le catalogue hors ligne.
pub(super) fn disabled_by_user() -> Vec<String> {
    crate::store::rule_overrides()
        .into_iter()
        .filter(|(_, enabled)| !enabled)
        .map(|(id, _)| id)
        .collect()
}

/// Catalogue affiché sur l'écran « Règles ». Bloquant : hors du fil d'interface.
pub fn catalog() -> RuleCatalog {
    let official: Vec<Rule> = fetch_official()
        .into_iter()
        .filter_map(RemoteRule::into_rule)
        .collect();
    let mut catalog = if official.is_empty() {
        // Même repli que l'analyse : catalogue du moteur.
        RuleCatalog::new(RuleOrigin::Builtin, builtin())
    } else {
        RuleCatalog::new(RuleOrigin::Online, official)
    };
    catalog.apply_overrides(&crate::store::rule_overrides());
    catalog
}

/// Règles `.herald` embarquées et règles écrites en Rust du moteur.
fn builtin() -> Vec<Rule> {
    herald_engine::dsl::builtin_rules()
        .into_iter()
        .chain(herald_engine::rules::extra_rules())
        .filter_map(|rule: BuiltinRule| {
            let source = effective_source(&rule.id, rule.source.to_owned());
            Some(Rule {
                id: rule.id.into(),
                category: Category::from_engine(rule.category)?,
                severity: Severity::from_engine(rule.severity),
                source,
                message: rule.message.into(),
                enabled: true,
                default_enabled: true,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_covers_every_engine_rule() {
        let engine_count = herald_engine::rules::list().len();
        let rules = builtin();
        assert!(!rules.is_empty());
        assert_eq!(rules.len(), engine_count, "aucune catégorie inconnue");
    }

    #[test]
    fn reads_the_official_shape() {
        let official: RemoteRule = serde_json::from_str(
            r#"{"id":"no-eval","category":"security","severity":"error","enabled":true,
                "source":"rule no-eval {}","usageCount":3,"message":"eval"}"#,
        )
        .unwrap();
        let rule = official.into_rule().unwrap();
        assert_eq!(rule.id.as_ref(), "no-eval");
        assert_eq!(rule.severity, Severity::Critical);
    }

    #[test]
    fn rust_rules_are_not_compiled_as_dsl() {
        // Source servie par herald.codes pour une règle écrite en Rust.
        let rust = "// Règle Rust — pas de fichier .herald (analyse cross-fichiers ou routée par nom de fichier plutôt que par langage).";
        assert!(!has_dsl_rule(rust));
        assert!(!has_dsl_rule("  \n// a\n\n   // b\n"));
        assert!(has_dsl_rule("// Désactive eval\nrule no-eval {}"));
        assert!(has_dsl_rule(&local_rules::builtin_sources()[0]));
    }
}
