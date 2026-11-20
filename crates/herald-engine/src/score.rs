use std::collections::HashMap;

use crate::types::{Grade, Issue, Score, Severity};

/// Facteur calibré sur demo-repo (dense → F) vs ansible (grand mais sain → B).
/// S'applique aux warnings/infos (voir [`density_penalty`]) — pas aux erreurs,
/// qui ont leur propre pénalité fixe ([`ERROR_PENALTY_PER_ISSUE`]).
pub const PENALTY_PER_DENSITY: f64 = 2.375;

/// Plafond de pénalité qu'une SEULE règle peut retirer au score global. Sans lui,
/// une règle très bruyante — p. ex. `no-unwrap-in-prod` sur un dépôt Rust, ou un
/// smell qui matche des centaines de fois — suffit à elle seule à faire chuter la
/// note à 0, ce qui écrase tout le reste du signal. Avec ce plafond, il faut
/// plusieurs familles de problèmes distinctes pour couler un dépôt.
///
/// Volontairement bas (15, pas 25+) : avec le coût fixe par erreur (voir
/// [`ERROR_PENALTY_PER_ISSUE`]), un plafond trop haut faisait qu'à peine 4
/// règles chargées en même temps atteignaient déjà 100 de pénalité — n'importe
/// quel gros dépôt un peu chargé sur 4 catégories tombait au plancher, sans
/// distinguer « pas mal de soucis » de « vraiment catastrophique ». Avec 15,
/// il en faut sept, toutes bien chargées, pour couler vraiment.
pub const MAX_RULE_PENALTY: f64 = 15.0;

/// Pénalité FIXE par occurrence d'une erreur (severity critique), indépendante
/// du nombre de fichiers. Un secret leaké ou une faille SSRF est grave dans
/// l'absolu, que le dépôt fasse 10 ou 5000 fichiers — contrairement aux
/// warnings/infos (jugés par densité, où « beaucoup de lignes longues sur un
/// petit repo » a plus de sens à peser que sur un gros), diviser la gravité
/// d'une erreur par le nombre de fichiers du dépôt la rendait invisible sur
/// les gros monorepos (des centaines de secrets committés pouvaient encore
/// ressortir en note A). Reste plafonnée par [`MAX_RULE_PENALTY`] comme le
/// reste : il faut plusieurs règles chargées pour couler la note, pas une
/// seule répétée mécaniquement.
pub const ERROR_PENALTY_PER_ISSUE: f64 = 3.0;

/// Score plancher : un 0/100 pile sonne comme un calcul cassé plutôt que
/// « vraiment mauvais », même pour un dépôt criblé de problèmes sur plusieurs
/// règles à la fois. La lettre reste F en dessous de 40 de toute façon — ce
/// plancher évite juste le chiffre absolu.
pub const MIN_SCORE: f64 = 5.0;

/// Pénalité de densité d'un lot de warnings/infos (gravité rapportée au
/// nombre de fichiers). Les erreurs sont traitées à part, voir
/// [`ERROR_PENALTY_PER_ISSUE`].
fn density_penalty(warnings: u32, infos: u32, files: usize) -> f64 {
    let files = files.max(1) as f64;
    let weight = 2.0 * warnings as f64 + 0.5 * infos as f64;
    PENALTY_PER_DENSITY * weight / files
}

/// Pénalité totale d'un lot (une règle) : fixe pour les erreurs + densité pour
/// le reste, plafonnée à [`MAX_RULE_PENALTY`].
fn rule_penalty(errors: u32, warnings: u32, infos: u32, files: usize) -> f64 {
    (ERROR_PENALTY_PER_ISSUE * errors as f64 + density_penalty(warnings, infos, files)).min(MAX_RULE_PENALTY)
}

pub fn overall_score<I>(by_rule: I, files: usize) -> u32
where
    I: IntoIterator<Item = (u32, u32, u32)>,
{
    let penalty: f64 = by_rule
        .into_iter()
        .map(|(e, w, i)| rule_penalty(e, w, i, files))
        .sum();
    (100.0 - penalty).clamp(MIN_SCORE, 100.0).round() as u32
}

/// Lettre (+/-) correspondant à un score — chaque palier A-D est divisé en
/// trois tiers (bas → `-`, milieu → nu, haut → `+`), F reste indivisé.
pub fn grade_of(value: u32) -> Grade {
    match value {
        97..=100 => Grade::APlus,
        93..=96 => Grade::A,
        90..=92 => Grade::AMinus,
        85..=89 => Grade::BPlus,
        80..=84 => Grade::B,
        75..=79 => Grade::BMinus,
        70..=74 => Grade::CPlus,
        65..=69 => Grade::C,
        60..=64 => Grade::CMinus,
        54..=59 => Grade::DPlus,
        47..=53 => Grade::D,
        40..=46 => Grade::DMinus,
        _ => Grade::F,
    }
}

/// Score d'un dépôt à partir de tous ses relevés. Regroupe par règle (pour le
/// plafond par règle), calcule le global plafonné, et conserve les totaux
/// err/warn/info pour l'affichage.
pub fn calculate<'a>(issues: impl Iterator<Item = &'a Issue>, files: usize) -> Score {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    // (errors, warnings, infos) par règle — base du plafond par règle.
    let mut by_rule: HashMap<&'a str, (u32, u32, u32)> = HashMap::new();

    for issue in issues {
        let slot = by_rule.entry(issue.rule.as_str()).or_default();
        match issue.severity {
            Severity::Error => {
                errors += 1;
                slot.0 += 1;
            }
            Severity::Warning => {
                warnings += 1;
                slot.1 += 1;
            }
            Severity::Info => {
                infos += 1;
                slot.2 += 1;
            }
        }
    }

    let value = overall_score(by_rule.into_values(), files);

    Score {
        value,
        grade: grade_of(value),
        errors,
        warnings,
        infos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_noisy_rule_is_capped_not_zero() {
        // Une seule règle qui matche des centaines de fois : plafonnée, donc la
        // note ne tombe pas à 0 à elle seule.
        let value = overall_score([(739, 0, 0)], 120);
        assert_eq!(value, (100.0 - MAX_RULE_PENALTY).round() as u32); // 85
    }

    #[test]
    fn several_loaded_rules_still_sink() {
        // Plusieurs règles chargées cumulent leurs plafonds → effondrement (grade F).
        let value = overall_score(
            [(739, 0, 0), (300, 0, 0), (200, 0, 0), (100, 0, 0), (50, 0, 0)],
            120,
        );
        assert!(value < 40, "value={value}");
        // Assez de règles saturées (7 × plafond > 100) → le plancher, jamais 0 pile.
        let floor = overall_score([(999, 0, 0); 7], 120);
        assert_eq!(floor, MIN_SCORE as u32);
    }

    #[test]
    fn clean_repo_is_perfect() {
        assert_eq!(overall_score(std::iter::empty(), 100), 100);
    }

    #[test]
    fn small_repo_riddled_with_errors_still_collapses() {
        // Plusieurs règles d'erreurs chargées sur un petit repo → s'effondre
        // largement sous la moyenne (grade D/F), sans forcément toucher le
        // plancher — 4 règles chargées, ça reste "vraiment mauvais", pas
        // "catastrophique sur tous les fronts".
        let value = overall_score([(20, 5, 0), (18, 4, 1), (15, 3, 0), (12, 2, 0)], 12);
        assert!(value < 45, "value={value}");
    }

    #[test]
    fn score_never_hits_a_bare_zero() {
        let value = overall_score([(9999, 9999, 9999); 20], 1);
        assert_eq!(value, MIN_SCORE as u32, "value={value}");
    }

    #[test]
    fn errors_are_not_diluted_on_large_repos() {
        // Le bug qu'on corrige : avant, une seule occurrence d'erreur sur un
        // monorepo de 5000 fichiers se diluait à une pénalité quasi nulle
        // (score ~100 malgré un vrai problème critique). Maintenant, chaque
        // occurrence coûte un point fixe (`ERROR_PENALTY_PER_ISSUE`), visible
        // même sur un très gros dépôt.
        let value = overall_score([(1, 0, 0)], 5000);
        assert_eq!(value, 97, "value={value}"); // 100 - 3

        // Mais il faut un vrai cumul (plusieurs règles chargées) pour couler la
        // note — un gros dépôt avec des soucis répartis sur seulement 3-4
        // catégories ne doit pas tomber au plancher (voir la baisse de
        // `MAX_RULE_PENALTY`/`ERROR_PENALTY_PER_ISSUE` : avant, 4 règles
        // suffisaient à couler n'importe quel gros dépôt un peu chargé).
        let moderately_messy = overall_score(
            [(4, 0, 0), (4, 0, 0), (4, 0, 0), (4, 0, 0)],
            5000,
        );
        assert!(moderately_messy > 40, "value={moderately_messy}");
    }

    #[test]
    fn warnings_and_infos_still_scale_with_repo_size() {
        // Zéro erreur, beaucoup de warnings sur un gros dépôt → reste haut :
        // la densité reste pertinente pour ce qui n'est pas critique.
        let value = overall_score([(0, 208, 0)], 1764);
        assert!(value >= 95, "value={value}");
    }

    #[test]
    fn many_infos_dont_sink_the_score() {
        assert!(overall_score([(0, 0, 100)], 50) >= 98);
    }

    #[test]
    fn grades_map_to_letters() {
        assert!(matches!(grade_of(100), Grade::APlus));
        assert!(matches!(grade_of(94), Grade::A));
        assert!(matches!(grade_of(90), Grade::AMinus));
        assert!(matches!(grade_of(85), Grade::BPlus));
        assert!(matches!(grade_of(80), Grade::B));
        assert!(matches!(grade_of(75), Grade::BMinus));
        assert!(matches!(grade_of(70), Grade::CPlus));
        assert!(matches!(grade_of(65), Grade::C));
        assert!(matches!(grade_of(60), Grade::CMinus));
        assert!(matches!(grade_of(54), Grade::DPlus));
        assert!(matches!(grade_of(47), Grade::D));
        assert!(matches!(grade_of(40), Grade::DMinus));
        assert!(matches!(grade_of(39), Grade::F));
    }

    #[test]
    fn grade_labels_include_plus_minus() {
        assert_eq!(grade_of(100).label(), "A+");
        assert_eq!(grade_of(90).label(), "A-");
        assert_eq!(grade_of(39).label(), "F");
    }
}
