use gpui::SharedString;
use serde::{Deserialize, Serialize};

/// Gravité d'un problème, de la plus à la moins grave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl Severity {
    /// Gravité du moteur et de l'API (`error`, `warning`, `info`).
    pub fn from_engine(name: &str) -> Self {
        match name {
            "error" => Self::Critical,
            "warning" => Self::Warning,
            _ => Self::Info,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critique",
            Self::Warning => "Attention",
            Self::Info => "Info",
        }
    }
}

/// Catégorie de règle du moteur, avec les mêmes libellés que herald.codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Security,
    Quality,
    Git,
    Devops,
    Pattern,
    Dependencies,
    Tests,
}

impl Category {
    /// Ordre d'affichage du backend (`CATEGORY_ORDER` dans `response.rs`).
    pub const ALL: [Self; 7] = [
        Self::Security,
        Self::Quality,
        Self::Git,
        Self::Devops,
        Self::Pattern,
        Self::Dependencies,
        Self::Tests,
    ];

    /// Nom de catégorie du moteur et de l'API (`Category::name` de `herald-engine`).
    pub fn from_engine(name: &str) -> Option<Self> {
        Some(match name {
            "security" => Self::Security,
            "quality" => Self::Quality,
            "git" => Self::Git,
            "devops" => Self::Devops,
            "pattern" => Self::Pattern,
            "dependencies" => Self::Dependencies,
            "tests" | "testing" => Self::Tests,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Security => "Sécurité",
            Self::Quality => "Qualité",
            Self::Git => "Versioning",
            Self::Devops => "DevOps",
            Self::Pattern => "Pattern",
            Self::Dependencies => "Dépendances",
            Self::Tests => "Tests",
        }
    }
}

/// Un problème détecté par une règle à un emplacement précis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub rule: SharedString,
    pub title: SharedString,
    pub severity: Severity,
    pub category: Category,
    /// Chemin relatif à la racine du projet (`(git)` pour l'historique Git).
    pub file: SharedString,
    /// Ligne (1-indexée) ; `None` pour un problème qui porte sur tout le fichier ou le dépôt.
    pub line: Option<u32>,
    pub explanation: Option<SharedString>,
    pub fix: Option<SharedString>,
    /// Références externes (CWE, OWASP…).
    pub references: Option<SharedString>,
    /// Relevé d'une règle trop imprécise sur le corpus de mesure : à vérifier
    /// à la main, hors note (voir `engine::local_rules`).
    #[serde(default)]
    pub review: bool,
}

impl Issue {
    /// Emplacement lisible : `fichier:ligne` ou seulement le fichier.
    pub fn location(&self) -> SharedString {
        match self.line {
            Some(line) => format!("{}:{line}", self.file).into(),
            None => self.file.clone(),
        }
    }
}

/// Titre lisible tiré de l'identifiant d'une règle, comme sur herald.codes
/// (`leaked-secret` → « Leaked secret »).
pub fn humanize_rule(id: &str) -> String {
    let text = id.replace(['-', '_'], " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_are_humanized() {
        assert_eq!(humanize_rule("leaked-secret"), "Leaked secret");
        assert_eq!(humanize_rule("ai_duplicate_block"), "Ai duplicate block");
    }
}
