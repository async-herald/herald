//! Patterns de conception suivis par Herald (catégorie « Pattern » du moteur) :
//! pour chacun, les règles qui le vérifient, un schéma « à éviter / à faire »,
//! pourquoi il compte et deux astuces. L'onglet « Patterns » d'un projet en fait
//! une boîte par pattern, avec les endroits du code qui s'en écartent.
//!
//! Les textes évitent « : » au milieu d'une phrase : le retour à la ligne de
//! GPUI ne connaît pas l'espace insécable et isolerait les deux-points.

use super::{AnalysisReport, Category};

/// Schéma : des chaînes de blocs reliés par des flèches (`A → B → C`), une par ligne.
pub type Diagram = &'static [&'static [&'static str]];

/// Un pattern à respecter.
pub struct Pattern {
    pub name: &'static str,
    /// Icône (chemin d'asset, `assets/icons/pattern-*.svg`).
    pub icon: &'static str,
    /// Règles du moteur qui le vérifient (identifiants).
    pub rules: &'static [&'static str],
    /// Pourquoi il compte, en une phrase.
    pub why: &'static str,
    pub avoid: Diagram,
    pub prefer: Diagram,
    pub tips: &'static [&'static str],
}

pub const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "Responsabilité unique",
        icon: "icons/pattern-single.svg",
        rules: &["solid", "god-class"],
        why: "Une classe qui fait une seule chose se comprend, se teste et se modifie sans risque.",
        avoid: &[
            &["UserManager", "BDD"],
            &["UserManager", "Email"],
            &["UserManager", "PDF"],
        ],
        prefer: &[
            &["UserRepository", "BDD"],
            &["Mailer", "Email"],
            &["InvoicePdf", "PDF"],
        ],
        tips: &[
            "Si la décrire demande un « et », découpez la classe.",
            "Au-delà d'une vingtaine de méthodes, regroupez-les par données manipulées.",
        ],
    },
    Pattern {
        name: "Ne pas se répéter (DRY)",
        icon: "icons/pattern-dry.svg",
        rules: &["dry"],
        why: "Une logique copiée diverge vite, et un correctif n'arrive pas partout.",
        avoid: &[
            &["checkout()", "calcul TVA"],
            &["facture()", "calcul TVA (copie)"],
        ],
        prefer: &[&["checkout()", "tva()"], &["facture()", "tva()"]],
        tips: &[
            "Dès la deuxième copie, extrayez une fonction nommée d'après ce qu'elle fait.",
            "Ne fusionnez pas deux codes qui changent pour des raisons différentes.",
        ],
    },
    Pattern {
        name: "Couches séparées (MVC)",
        icon: "icons/pattern-layers.svg",
        rules: &["mvc"],
        why: "Données, logique et affichage mêlés empêchent de changer l'un sans casser l'autre.",
        avoid: &[&["Contrôleur", "SQL + HTML"]],
        prefer: &[
            &["Contrôleur", "Modèle", "BDD"],
            &["Contrôleur", "Vue", "HTML"],
        ],
        tips: &[
            "Le contrôleur lit la requête, appelle un service et renvoie la réponse.",
            "Les vues reçoivent des données prêtes, sans requête ni calcul métier.",
        ],
    },
    Pattern {
        name: "Faible couplage",
        icon: "icons/pattern-unlink.svg",
        rules: &["high-coupling", "feature-envy"],
        why: "Un module qui dépend de tout casse dès que l'un de ses voisins change.",
        avoid: &[
            &["checkout", "panier"],
            &["checkout", "stock"],
            &["checkout", "paiement"],
            &["checkout", "emails"],
        ],
        prefer: &[
            &["checkout", "CommandeService"],
            &["CommandeService", "panier · stock · paiement"],
        ],
        tips: &[
            "Au-delà d'une quinzaine d'imports, le module a sans doute plusieurs rôles.",
            "Déplacez une méthode vers l'objet dont elle utilise surtout les données.",
        ],
    },
    Pattern {
        name: "Frontières de modules",
        icon: "icons/pattern-modules.svg",
        rules: &[
            "deep-import-internals",
            "deep-relative-import",
            "barrel-wildcard-reexport",
        ],
        why: "Importer les fichiers internes d'un module empêche de le réorganiser.",
        avoid: &[&["billing/view.js", "lib/internal/utils.js"]],
        prefer: &[&["billing/view.js", "lib", "utils.js"]],
        tips: &[
            "Importez un module par son point d'entrée public (index, mod.rs, __init__).",
            "Remplacez les chemins en ../../../ par des alias ou des imports absolus.",
        ],
    },
    Pattern {
        name: "État partagé maîtrisé",
        icon: "icons/pattern-state.svg",
        rules: &["global-mutable-state"],
        why: "Une variable globale modifiable change de n'importe où, et les bugs deviennent imprévisibles.",
        avoid: &[&["a()", "compteur global"], &["b()", "compteur global"]],
        prefer: &[
            &["main()", "Compteur"],
            &["Compteur", "a(compteur)"],
            &["Compteur", "b(compteur)"],
        ],
        tips: &[
            "Passez l'état en paramètre ou dans un objet créé une seule fois.",
            "Ne gardez au niveau du module que des constantes.",
        ],
    },
    Pattern {
        name: "Dépendances injectées",
        icon: "icons/pattern-inject.svg",
        rules: &["symfony-service-locator"],
        why: "Une classe qui va chercher ses services dans le conteneur cache ses dépendances.",
        avoid: &[&["Service", "conteneur.get(Mailer)"]],
        prefer: &[&["Mailer", "constructeur", "Service"]],
        tips: &[
            "Recevez chaque service dans le constructeur.",
            "En test, passez de simples substituts sans démarrer le framework.",
        ],
    },
];

/// Nom et icône de l'entrée « Autres » (règles de pattern hors catalogue).
pub const OTHERS_NAME: &str = "Autres règles de pattern";
pub const OTHERS_ICON: &str = "icons/pattern-other.svg";

/// Endroit du code qui s'écarte d'un pattern.
pub struct PatternFinding {
    /// Indice dans `AnalysisReport::issues`.
    pub issue: usize,
}

/// Bilan d'un pattern pour un rapport.
pub struct PatternStatus {
    /// `None` : règles de la catégorie Pattern absentes du catalogue (« Autres »).
    pub pattern: Option<&'static Pattern>,
    pub findings: Vec<PatternFinding>,
}

impl PatternStatus {
    /// Nom affiché, qui sert aussi d'identifiant stable (page ouverte).
    pub fn name(&self) -> &'static str {
        self.pattern.map_or(OTHERS_NAME, |pattern| pattern.name)
    }

    pub fn icon(&self) -> &'static str {
        self.pattern.map_or(OTHERS_ICON, |pattern| pattern.icon)
    }

    pub fn is_respected(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Bilan de chaque pattern : d'abord ceux qui ont des endroits à revoir (les
/// plus touchés en tête), puis ceux qui sont respectés. Les problèmes marqués
/// comme corrigés ne comptent pas. Les règles de la catégorie Pattern absentes
/// du catalogue forment une dernière entrée « Autres », s'il y en a.
pub fn statuses(report: &AnalysisReport) -> Vec<PatternStatus> {
    let open = |ix: usize| !report.is_resolved(ix);
    let mut statuses: Vec<PatternStatus> = PATTERNS
        .iter()
        .map(|pattern| PatternStatus {
            pattern: Some(pattern),
            findings: report
                .issues
                .iter()
                .enumerate()
                .filter(|(ix, issue)| open(*ix) && pattern.rules.contains(&issue.rule.as_ref()))
                .map(|(issue, _)| PatternFinding { issue })
                .collect(),
        })
        .collect();
    statuses.sort_by_key(|status| std::cmp::Reverse(status.findings.len()));

    let known = |rule: &str| PATTERNS.iter().any(|pattern| pattern.rules.contains(&rule));
    let others: Vec<PatternFinding> = report
        .issues
        .iter()
        .enumerate()
        .filter(|(ix, issue)| {
            open(*ix) && issue.category == Category::Pattern && !known(issue.rule.as_ref())
        })
        .map(|(issue, _)| PatternFinding { issue })
        .collect();
    if !others.is_empty() {
        statuses.push(PatternStatus {
            pattern: None,
            findings: others,
        });
    }
    statuses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_belongs_to_a_single_pattern() {
        let mut seen = std::collections::HashSet::new();
        for pattern in PATTERNS {
            assert!(!pattern.tips.is_empty(), "{} sans astuce", pattern.name);
            assert!(
                !pattern.avoid.is_empty() && !pattern.prefer.is_empty(),
                "{} sans schéma",
                pattern.name
            );
            for rule in pattern.rules {
                assert!(seen.insert(*rule), "{rule} dans deux patterns");
            }
        }
    }

    #[test]
    fn texts_have_no_colon_mid_sentence() {
        for pattern in PATTERNS {
            for text in std::iter::once(pattern.why).chain(pattern.tips.iter().copied()) {
                assert!(!text.contains(" :"), "« : » dans {text:?}");
            }
        }
    }
}
