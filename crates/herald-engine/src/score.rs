use std::collections::HashSet;

use crate::types::{Grade, Issue, Score, Severity};

pub fn calculate<'a>(issues: impl Iterator<Item = &'a Issue>) -> Score {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;

    // Un seul type de point par problème : on regroupe les occurrences similaires
    // (même règle = même problème) et chaque problème distinct retire 1 point,
    // quelle que soit sa sévérité. Les compteurs errors/warnings/infos gardent eux
    // le total des occurrences (affichage).
    let mut rules: HashSet<&'a str> = HashSet::new();

    for issue in issues {
        match issue.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => infos += 1,
        }
        rules.insert(issue.rule.as_str());
    }

    let penalty = rules.len() as u32;
    let value = 100u32.saturating_sub(penalty);

    let grade = match value {
        90..=100 => Grade::A,
        75..=89  => Grade::B,
        60..=74  => Grade::C,
        40..=59  => Grade::D,
        _        => Grade::F,
    };

    Score {
        value,
        grade,
        errors,
        warnings,
        infos,
    }
}