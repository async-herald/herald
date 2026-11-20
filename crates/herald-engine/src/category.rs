use serde::{Deserialize, Serialize};

/// Les 7 catégories du produit. `Architecture`, `Performance` et `Reliability`
/// ont été retirées : leurs règles sont désormais rangées en `Pattern` (ex-
/// architecture) ou `Quality` (ex-performance/reliability, ex-code-smell) —
/// voir `dsl::parser::maps::parse_category` pour les alias conservés.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Security,
    Quality,
    Dependencies,
    Git,
    Pattern,
    Testing,
    Devops,
}

impl Category {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Security     => "security",
            Self::Quality      => "quality",
            Self::Dependencies => "dependencies",
            Self::Git          => "git",
            Self::Pattern      => "pattern",
            Self::Testing      => "tests",
            Self::Devops       => "devops",
        }
    }

    /// Catégories réservées aux offres payantes (Pro/Business). La note reste
    /// visible pour un compte gratuit, mais pas le détail des relevés — les
    /// handlers d'analyse et de lecture de rapport doivent masquer `issues`
    /// côté serveur pour ces catégories, pas seulement dans l'UI (sinon un
    /// compte gratuit voit tout en inspectant la réponse réseau brute).
    pub const fn is_pro(&self) -> bool {
        matches!(self, Self::Pattern | Self::Git | Self::Testing | Self::Devops)
    }
}

/// Même règle que [`Category::is_pro`], à partir du nom de catégorie stocké
/// en base (`findings.category`, une chaîne libre plutôt que l'enum) — sert à
/// filtrer les rapports archivés relus depuis la base.
pub fn category_name_is_pro(name: &str) -> bool {
    matches!(name, "pattern" | "git" | "tests" | "devops")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pro_matches_category_name_is_pro() {
        let all = [
            Category::Security,
            Category::Quality,
            Category::Dependencies,
            Category::Git,
            Category::Pattern,
            Category::Testing,
            Category::Devops,
        ];
        for cat in all {
            assert_eq!(
                cat.is_pro(),
                category_name_is_pro(cat.name()),
                "désaccord pour {}",
                cat.name()
            );
        }
    }

    #[test]
    fn exactly_four_pro_categories() {
        let all = [
            Category::Security,
            Category::Quality,
            Category::Dependencies,
            Category::Git,
            Category::Pattern,
            Category::Testing,
            Category::Devops,
        ];
        assert_eq!(all.iter().filter(|c| c.is_pro()).count(), 4);
    }
}
