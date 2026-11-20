use gpui::SharedString;

use super::{Category, Severity};

/// Règle d'analyse officielle, telle qu'affichée sur l'écran « Règles ».
#[derive(Clone, Debug)]
pub struct Rule {
    pub id: SharedString,
    pub category: Category,
    pub severity: Severity,
    pub message: SharedString,
    /// Source `.herald` de la règle (ou une note pour les règles écrites en Rust).
    pub source: SharedString,
    /// Appliquée par l'analyse : choix de l'utilisateur, sinon l'état par défaut.
    pub enabled: bool,
    /// État par défaut : activée sur herald.codes (ou dans le moteur hors ligne).
    pub default_enabled: bool,
}

impl Rule {
    /// L'utilisateur a changé l'état par défaut de cette règle.
    pub fn is_overridden(&self) -> bool {
        self.enabled != self.default_enabled
    }
}

/// D'où vient la liste des règles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleOrigin {
    /// Catalogue en ligne de herald.codes.
    Online,
    /// Catalogue intégré au moteur, utilisé hors ligne.
    Builtin,
}

/// Règles officielles, dans l'ordre d'affichage.
#[derive(Clone, Debug)]
pub struct RuleCatalog {
    pub origin: RuleOrigin,
    pub rules: Vec<Rule>,
}

impl RuleCatalog {
    pub fn new(origin: RuleOrigin, mut rules: Vec<Rule>) -> Self {
        // Catégorie (ordre du backend), puis gravité, puis identifiant.
        rules.sort_by(|a, b| {
            category_rank(a.category)
                .cmp(&category_rank(b.category))
                .then(a.severity.cmp(&b.severity))
                .then(a.id.cmp(&b.id))
        });
        Self { origin, rules }
    }

    /// Règles de la catégorie (`None` : toutes).
    pub fn in_category(&self, filter: Option<Category>) -> impl Iterator<Item = &Rule> {
        self.rules
            .iter()
            .filter(move |rule| filter.is_none_or(|category| rule.category == category))
    }

    pub fn active_count(&self) -> usize {
        self.rules.iter().filter(|rule| rule.enabled).count()
    }

    /// Nombre de règles dont l'utilisateur a changé l'état.
    pub fn overridden_count(&self) -> usize {
        self.rules
            .iter()
            .filter(|rule| rule.is_overridden())
            .count()
    }

    /// Applique les choix de l'utilisateur (`store::rule_overrides`).
    pub fn apply_overrides(&mut self, overrides: &std::collections::HashMap<String, bool>) {
        for rule in &mut self.rules {
            rule.enabled = overrides
                .get(rule.id.as_ref())
                .copied()
                .unwrap_or(rule.default_enabled);
        }
    }

    /// Active ou désactive une règle ; renvoie le choix à enregistrer
    /// (`None` : la règle revient à son état par défaut).
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Option<Option<bool>> {
        let rule = self.rules.iter_mut().find(|rule| rule.id.as_ref() == id)?;
        rule.enabled = enabled;
        Some((enabled != rule.default_enabled).then_some(enabled))
    }

    /// Remet toutes les règles dans leur état par défaut.
    pub fn reset(&mut self) {
        for rule in &mut self.rules {
            rule.enabled = rule.default_enabled;
        }
    }
}

fn category_rank(category: Category) -> usize {
    Category::ALL
        .iter()
        .position(|other| *other == category)
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_choices_override_the_default_state() {
        let mut catalog = RuleCatalog::new(
            RuleOrigin::Online,
            vec![
                rule("eval-usage", Category::Security, Severity::Critical, true),
                rule("long-line", Category::Quality, Severity::Info, false),
            ],
        );
        assert_eq!(catalog.set_enabled("eval-usage", false), Some(Some(false)));
        assert_eq!(catalog.active_count(), 0);
        assert_eq!(catalog.overridden_count(), 1);
        // Revenir à l'état par défaut efface le choix.
        assert_eq!(catalog.set_enabled("eval-usage", true), Some(None));
        catalog.apply_overrides(&std::collections::HashMap::from([(
            "long-line".to_owned(),
            true,
        )]));
        assert_eq!(catalog.active_count(), 2);
        catalog.reset();
        assert_eq!(catalog.active_count(), 1);
        assert_eq!(catalog.set_enabled("inconnue", true), None);
    }

    fn rule(id: &str, category: Category, severity: Severity, enabled: bool) -> Rule {
        Rule {
            id: id.to_owned().into(),
            category,
            severity,
            message: SharedString::default(),
            source: SharedString::default(),
            enabled,
            default_enabled: enabled,
        }
    }

    #[test]
    fn sorts_by_category_then_severity_and_filters() {
        let catalog = RuleCatalog::new(
            RuleOrigin::Builtin,
            vec![
                rule("b", Category::Quality, Severity::Info, true),
                rule("a", Category::Quality, Severity::Critical, false),
                rule("z", Category::Security, Severity::Info, true),
            ],
        );
        let ids: Vec<&str> = catalog.in_category(None).map(|r| r.id.as_ref()).collect();
        assert_eq!(ids, ["z", "a", "b"]);
        assert_eq!(catalog.in_category(Some(Category::Quality)).count(), 2);
        assert_eq!(catalog.active_count(), 2);
    }
}
