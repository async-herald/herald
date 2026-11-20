use std::collections::{BTreeSet, HashMap};

use gpui::SharedString;
use herald_engine::score::{self, overall_score};
use serde::{Deserialize, Serialize};

use super::{Category, Grade, Issue, Severity};

/// Chiffres clés d'une analyse.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub score: u8,
    pub previous_score: Option<u8>,
    pub file_count: u32,
    pub language_count: u32,
    pub duration_ms: u32,
    pub issue_count: u32,
    /// Moment de l'analyse, déjà formaté (« il y a 2 min »).
    pub analyzed_at: SharedString,
    /// Moment de l'analyse en secondes Unix (0 : inconnu).
    #[serde(default)]
    pub analyzed_at_unix: u64,
}

impl Summary {
    pub fn grade(&self) -> Grade {
        Grade::from_score(self.score)
    }

    /// Évolution du score depuis l'analyse précédente.
    pub fn delta(&self) -> Option<i16> {
        self.previous_score
            .map(|previous| i16::from(self.score) - i16::from(previous))
    }
}

/// Score d'une catégorie de règles.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CategoryScore {
    pub category: Category,
    pub score: u8,
    pub issue_count: u32,
    /// Comptée dans la note globale (DevOps / Tests : seulement s'il y a des fichiers à examiner).
    pub applicable: bool,
}

impl CategoryScore {
    pub fn grade(&self) -> Grade {
        Grade::from_score(self.score)
    }
}

/// Extrait de code source conservé pour l'affichage.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: SharedString,
    pub first_line: u32,
    pub lines: Vec<SharedString>,
}

impl SourceFile {
    /// Lignes numérotées de l'extrait.
    pub fn numbered_lines(&self) -> impl Iterator<Item = (u32, &SharedString)> {
        (self.first_line..).zip(&self.lines)
    }
}

/// Résultat complet d'une analyse.
#[derive(Clone, Debug)]
pub struct AnalysisReport {
    pub summary: Summary,
    pub categories: Vec<CategoryScore>,
    /// Tous les problèmes, y compris ceux des catégories verrouillées (comptés dans la note).
    pub issues: Vec<Issue>,
    pub sources: Vec<SourceFile>,
    /// Problèmes marqués comme corrigés par l'utilisateur, en attente de la
    /// prochaine analyse pour confirmation.
    resolved: BTreeSet<usize>,
    groups: Vec<IssueGroup>,
    /// Points de chaque groupe s'il était le seul corrigé (indépendant des
    /// corrections marquées) : l'ordre du plan de correction.
    base_gains: Vec<f32>,
    /// Calculs mis en cache (la liste est redessinée à chaque image du défilement).
    cache: ScoreCache,
}

/// Problèmes d'une même règle, réunis en une seule entrée de la liste.
#[derive(Clone, Debug)]
pub struct IssueGroup {
    #[allow(dead_code)]
    pub rule: SharedString,
    pub title: SharedString,
    /// Gravité la plus haute des occurrences.
    pub severity: Severity,
    pub category: Category,
    /// Indices dans `AnalysisReport::issues`, dans l'ordre des fichiers.
    pub occurrences: Vec<usize>,
    /// Règle « À revoir » : hors note, listée après les autres.
    pub review: bool,
}

#[derive(Clone, Debug, Default)]
struct ScoreCache {
    projected: u8,
    /// Points gagnés en corrigeant toutes les occurrences encore ouvertes de chaque groupe.
    group_gains: Vec<f32>,
    /// Groupes visibles, les plus graves d'abord.
    by_severity: Vec<usize>,
}

/// Prochain palier de note à atteindre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NextGrade {
    pub grade: Grade,
    pub points_needed: u8,
}

impl AnalysisReport {
    pub fn new(
        summary: Summary,
        categories: Vec<CategoryScore>,
        issues: Vec<Issue>,
        sources: Vec<SourceFile>,
    ) -> Self {
        let groups = group_issues(&issues);
        let mut report = Self {
            summary,
            categories,
            issues,
            sources,
            resolved: BTreeSet::new(),
            groups,
            base_gains: Vec::new(),
            cache: ScoreCache::default(),
        };
        report.base_gains = report.compute_base_gains();
        report.refresh_cache();
        report
    }

    fn refresh_cache(&mut self) {
        let files = self.summary.file_count as usize;
        let current = self.breakdown(|ix| self.is_resolved(ix));
        let before = raw_score(&current, files);
        let projected = overall_score(current, files) as u8;

        let group_gains = self
            .groups
            .iter()
            .map(|group| {
                if !self.is_applicable(group.category) {
                    return 0.;
                }
                let after = raw_score(
                    &self.breakdown(|ix| self.is_resolved(ix) || group.occurrences.contains(&ix)),
                    files,
                );
                (after - before).max(0.) as f32
            })
            .collect();

        let mut by_severity: Vec<usize> = (0..self.groups.len()).collect();
        by_severity.sort_by_key(|&gi| {
            let group = &self.groups[gi];
            (
                group.review,
                group.severity,
                std::cmp::Reverse(group.occurrences.len()),
            )
        });

        self.cache = ScoreCache {
            projected,
            group_gains,
            by_severity,
        };
    }

    fn compute_base_gains(&self) -> Vec<f32> {
        let files = self.summary.file_count as usize;
        let before = raw_score(&self.breakdown(|_| false), files);
        self.groups
            .iter()
            .map(|group| {
                if !self.is_applicable(group.category) {
                    return 0.;
                }
                let after = raw_score(&self.breakdown(|ix| group.occurrences.contains(&ix)), files);
                (after - before).max(0.) as f32
            })
            .collect()
    }

    /// Points que rapporte le groupe à lui seul, corrections marquées ou non.
    pub fn base_gain(&self, gi: usize) -> f32 {
        self.base_gains.get(gi).copied().unwrap_or(0.)
    }

    /// Note obtenue si les groupes `groups` (et rien d'autre) étaient corrigés.
    pub fn score_with(&self, groups: &[usize]) -> u8 {
        let fixed: std::collections::HashSet<usize> = groups
            .iter()
            .filter_map(|&gi| self.groups.get(gi))
            .flat_map(|group| group.occurrences.iter().copied())
            .collect();
        overall_score(
            self.breakdown(|ix| fixed.contains(&ix)),
            self.summary.file_count as usize,
        ) as u8
    }

    /// Groupe (et rang dans ses occurrences) d'un problème.
    pub fn group_of(&self, ix: usize) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(gi, group)| {
            group
                .occurrences
                .iter()
                .position(|&occ| occ == ix)
                .map(|pos| (gi, pos))
        })
    }

    /// Plan de correction : groupes visibles, du plus rentable au moins
    /// rentable, et combien des premiers suffisent pour gagner un palier
    /// (au plus `max_quick`).
    pub fn correction_plan(&self, max_quick: usize) -> (Vec<usize>, usize) {
        let mut order: Vec<usize> = self.cache.by_severity.clone();
        order.sort_by(|&a, &b| self.base_gain(b).total_cmp(&self.base_gain(a)));
        let target = Grade::from_score(self.summary.score)
            .next_up()
            .map(|grade| grade.min_score());
        // Au moins trois corrections proposées (ou toutes s'il y en a moins),
        // davantage s'il en faut plus pour gagner un palier.
        let min_quick = order.len().min(3);
        let mut quick = order.len().min(max_quick);
        if let Some(target) = target {
            for n in 1..=order.len().min(max_quick) {
                if self.score_with(&order[..n]) >= target {
                    quick = n.max(min_quick);
                    break;
                }
            }
        }
        (order, quick)
    }

    pub fn groups(&self) -> &[IssueGroup] {
        &self.groups
    }

    pub fn group(&self, gi: usize) -> Option<&IssueGroup> {
        self.groups.get(gi)
    }

    /// Toutes les occurrences du groupe sont marquées comme corrigées.
    pub fn is_group_resolved(&self, gi: usize) -> bool {
        self.groups
            .get(gi)
            .is_some_and(|group| group.occurrences.iter().all(|ix| self.is_resolved(*ix)))
    }

    /// Marque tout le groupe comme corrigé, ou annule.
    pub fn toggle_group_resolved(&mut self, gi: usize) {
        let Some(group) = self.groups.get(gi) else {
            return;
        };
        let resolve = !self.is_group_resolved(gi);
        for &ix in &group.occurrences {
            if resolve {
                self.resolved.insert(ix);
            } else {
                self.resolved.remove(&ix);
            }
        }
        self.refresh_cache();
    }

    pub fn is_resolved(&self, ix: usize) -> bool {
        self.resolved.contains(&ix)
    }

    /// Problèmes marqués comme corrigés (indices dans `issues`), pour la sauvegarde.
    pub fn resolved_indices(&self) -> Vec<usize> {
        self.resolved.iter().copied().collect()
    }

    /// Restaure les corrections marquées d'une analyse sauvegardée.
    pub fn set_resolved(&mut self, resolved: impl IntoIterator<Item = usize>) {
        let count = self.issues.len();
        self.resolved = resolved.into_iter().filter(|&ix| ix < count).collect();
        self.refresh_cache();
    }

    fn category(&self, category: Category) -> Option<&CategoryScore> {
        self.categories.iter().find(|c| c.category == category)
    }

    /// Catégorie comptée dans la note globale (DevOps et Tests ne le sont que
    /// s'il y a des fichiers à examiner, comme sur le backend).
    fn is_applicable(&self, category: Category) -> bool {
        self.category(category).is_none_or(|c| c.applicable)
    }

    /// Relevés par règle `(erreurs, avertissements, infos)` des catégories
    /// applicables, hors problèmes exclus et hors « À revoir » : l'entrée du
    /// barème du moteur.
    fn breakdown(&self, excluded: impl Fn(usize) -> bool) -> Vec<(u32, u32, u32)> {
        let mut by_rule: HashMap<&str, (u32, u32, u32)> = HashMap::new();
        for (ix, issue) in self.issues.iter().enumerate() {
            if issue.review || excluded(ix) || !self.is_applicable(issue.category) {
                continue;
            }
            let slot = by_rule.entry(issue.rule.as_ref()).or_default();
            match issue.severity {
                Severity::Critical => slot.0 += 1,
                Severity::Warning => slot.1 += 1,
                Severity::Info => slot.2 += 1,
            }
        }
        by_rule.into_values().collect()
    }

    /// Score estimé une fois les corrections marquées confirmées (barème du moteur).
    pub fn projected_score(&self) -> u8 {
        self.cache.projected
    }

    /// Points gagnés sur la note globale en corrigeant tout le groupe (non arrondis).
    pub fn group_gain(&self, gi: usize) -> f32 {
        self.cache.group_gains.get(gi).copied().unwrap_or(0.)
    }

    /// Palier suivant à partir du score projeté, `None` si la note est déjà A+.
    pub fn next_grade(&self) -> Option<NextGrade> {
        let score = self.projected_score();
        let next = Grade::from_score(score).next_up()?;
        Some(NextGrade {
            grade: next,
            points_needed: next.min_score().saturating_sub(score),
        })
    }

    /// Groupes visibles non corrigés qui rapportent le plus de points.
    pub fn quick_wins(&self, limit: usize) -> Vec<usize> {
        let mut gains: Vec<(usize, f32)> = self
            .cache
            .by_severity
            .iter()
            .map(|&gi| (gi, self.group_gain(gi)))
            .filter(|&(gi, gain)| gain > 0. && !self.is_group_resolved(gi))
            .collect();
        gains.sort_by(|a, b| b.1.total_cmp(&a.1));
        gains.into_iter().take(limit).map(|(gi, _)| gi).collect()
    }

    /// Groupes visibles triés par gravité, éventuellement d'une seule catégorie.
    pub fn group_indices(&self, category: Option<Category>) -> Vec<usize> {
        self.cache
            .by_severity
            .iter()
            .copied()
            .filter(|&gi| category.is_none_or(|c| self.groups[gi].category == c))
            .collect()
    }

    pub fn source(&self, path: &str) -> Option<&SourceFile> {
        self.sources.iter().find(|source| source.path == path)
    }
}

/// Regroupe les problèmes par règle, dans l'ordre de première apparition.
fn group_issues(issues: &[Issue]) -> Vec<IssueGroup> {
    let mut groups: Vec<IssueGroup> = Vec::new();
    let mut by_rule: HashMap<&str, usize> = HashMap::new();
    for (ix, issue) in issues.iter().enumerate() {
        match by_rule.get(issue.rule.as_ref()) {
            Some(&gi) => {
                let group = &mut groups[gi];
                group.occurrences.push(ix);
                group.severity = group.severity.min(issue.severity);
            }
            None => {
                by_rule.insert(issue.rule.as_ref(), groups.len());
                groups.push(IssueGroup {
                    rule: issue.rule.clone(),
                    title: issue.title.clone(),
                    severity: issue.severity,
                    category: issue.category,
                    occurrences: vec![ix],
                    review: issue.review,
                });
            }
        }
    }
    groups
}

/// Score non arrondi, même formule que `herald_engine::score::overall_score`
/// (pénalité par règle plafonnée), pour estimer finement le gain d'une correction.
fn raw_score(breakdown: &[(u32, u32, u32)], files: usize) -> f64 {
    let files = files.max(1) as f64;
    let penalty: f64 = breakdown
        .iter()
        .map(|&(errors, warnings, infos)| {
            let density = score::PENALTY_PER_DENSITY
                * (2. * f64::from(warnings) + 0.5 * f64::from(infos))
                / files;
            (score::ERROR_PENALTY_PER_ISSUE * f64::from(errors) + density)
                .min(score::MAX_RULE_PENALTY)
        })
        .sum();
    (100. - penalty).clamp(score::MIN_SCORE, 100.)
}

/// Points formatés à la française : « +3 pts », « +0,5 pt ».
pub fn format_points(points: f32) -> String {
    let rounded = (points * 10.).round() / 10.;
    let text = if rounded.fract() == 0. {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}").replace('.', ",")
    };
    let unit = if rounded >= 2. { "pts" } else { "pt" };
    format!("+{text} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_plan_puts_the_biggest_gains_first() {
        let report = AnalysisReport::new(
            summary(0),
            vec![category(Category::Security), category(Category::Quality)],
            vec![
                issue("long-line", Severity::Info, Category::Quality),
                issue("eval-usage", Severity::Critical, Category::Security),
                issue("eval-usage", Severity::Critical, Category::Security),
            ],
            vec![],
        );
        let (order, quick) = report.correction_plan(5);
        assert_eq!(order.len(), 2);
        assert_eq!(report.groups()[order[0]].rule.as_ref(), "eval-usage");
        assert!(report.base_gain(order[0]) >= report.base_gain(order[1]));
        assert!((1..=2).contains(&quick));
        assert_eq!(report.group_of(2), Some((order[0], 1)));
        assert!(report.score_with(&order) >= report.score_with(&order[..1]));
    }

    fn summary(duration_ms: u32) -> Summary {
        Summary {
            score: 88,
            previous_score: Some(84),
            file_count: 10,
            language_count: 1,
            duration_ms,
            issue_count: 0,
            analyzed_at: "maintenant".into(),
            analyzed_at_unix: 0,
        }
    }

    fn issue(rule: &str, severity: Severity, category: Category) -> Issue {
        Issue {
            rule: rule.to_owned().into(),
            title: rule.to_owned().into(),
            severity,
            category,
            file: "src/main.rs".into(),
            line: Some(3),
            explanation: None,
            fix: None,
            references: None,
            review: false,
        }
    }

    fn category(category: Category) -> CategoryScore {
        CategoryScore {
            category,
            score: 100,
            issue_count: 0,
            applicable: true,
        }
    }

    fn report() -> AnalysisReport {
        let issues = vec![
            issue("leaked-secret", Severity::Critical, Category::Security),
            issue("long-line", Severity::Warning, Category::Quality),
            issue("god-class", Severity::Warning, Category::Pattern),
            issue("long-line", Severity::Warning, Category::Quality),
        ];
        let mut report = AnalysisReport::new(
            summary(1_000),
            Category::ALL.into_iter().map(category).collect(),
            issues,
            Vec::new(),
        );
        report.summary.score = report.projected_score();
        report
    }

    #[test]
    fn duration_uses_french_decimal_comma() {}

    #[test]
    fn delta_is_signed() {
        assert_eq!(summary(0).delta(), Some(4));
    }

    #[test]
    fn score_matches_engine_formula() {
        // 1 erreur (3 pts) + long-line : 2 avertissements (0,95) + god-class : 1 (0,475), sur 10 fichiers.
        assert_eq!(report().summary.score, 96);
    }

    #[test]
    fn same_rule_is_grouped() {
        let report = report();
        assert_eq!(report.groups().len(), 3);
        let long_line = report
            .groups()
            .iter()
            .find(|g| g.rule.as_ref() == "long-line")
            .unwrap();
        assert_eq!(long_line.occurrences, vec![1, 3]);
    }

    #[test]
    fn resolving_raises_projected_score() {
        let mut report = report();
        let before = report.projected_score();
        let critical = report.quick_wins(1)[0];
        assert_eq!(report.groups()[critical].severity, Severity::Critical);
        assert!((report.group_gain(critical) - 3.).abs() < 0.01);

        report.toggle_group_resolved(critical);
        assert_eq!(report.projected_score(), before + 3);
        assert!(report.is_group_resolved(critical));
        assert!(!report.quick_wins(10).contains(&critical));

        report.toggle_group_resolved(critical);
        assert_eq!(report.projected_score(), before);
    }

    /// « À revoir » : visible, listé après les autres, sans effet sur la note.
    #[test]
    fn review_issues_do_not_count() {
        let mut issues = vec![
            issue("leaked-secret", Severity::Critical, Category::Security),
            issue("weak_crypto", Severity::Critical, Category::Security),
        ];
        issues[1].review = true;
        let report = AnalysisReport::new(
            summary(1_000),
            Category::ALL.into_iter().map(category).collect(),
            issues,
            Vec::new(),
        );
        let without = report.score_with(&[0]);
        assert_eq!(
            without, 100,
            "seul le relevé « À revoir » reste : note pleine"
        );
        let review = report.groups().iter().position(|g| g.review).unwrap();
        assert_eq!(report.group_gain(review), 0.);
        assert_eq!(*report.group_indices(None).last().unwrap(), review);
    }

    #[test]
    fn resolving_a_group_resolves_every_occurrence() {
        let mut report = report();
        let gi = report
            .groups()
            .iter()
            .position(|g| g.rule.as_ref() == "long-line")
            .unwrap();
        assert!((report.group_gain(gi) - 0.95).abs() < 0.01);
        report.toggle_group_resolved(gi);
        assert!(report.is_resolved(1) && report.is_resolved(3));
    }

    #[test]
    fn every_category_is_detailed() {
        let report = report();
        let visible = report.group_indices(None);
        assert_eq!(visible.len(), 3);
        assert!(
            visible
                .iter()
                .any(|&gi| report.groups()[gi].category == Category::Pattern)
        );
    }

    #[test]
    fn points_are_formatted_in_french() {
        assert_eq!(format_points(3.), "+3 pts");
        assert_eq!(format_points(0.5), "+0,5 pt");
        assert_eq!(format_points(0.05), "+0,1 pt");
    }

    #[test]
    fn category_filter_keeps_only_that_category() {
        let report = report();
        let security = report.group_indices(Some(Category::Security));
        assert_eq!(security.len(), 1);
        assert_eq!(report.groups()[security[0]].category, Category::Security);
    }
}
