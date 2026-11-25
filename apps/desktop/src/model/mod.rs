//! Modèle de données affiché par l'interface.
//!
//! Ces types sont volontairement indépendants de GPUI (hors `SharedString`) ; ils
//! sont remplis par `crate::engine` à partir du résultat du moteur `herald-engine`.

mod advisory;
pub mod code_map;
mod grade;
mod issue;
pub mod patterns;
mod project;
mod report;
mod rule;
mod rule_text;
pub mod sample;
mod usage;

pub use advisory::Advisory;
#[cfg(test)]
pub(crate) use advisory::advisory_for_tests;
pub use grade::{Grade, GradeTone};
pub use issue::{Category, Issue, Severity, humanize_rule};
pub use project::Project;
pub use report::{
    AnalysisReport, CategoryScore, IssueGroup, PackageScore, SourceFile, Summary, format_points,
};
pub use rule::{Rule, RuleCatalog, RuleOrigin};
pub use rule_text::{rule_text, rule_title};
pub use usage::Usage;
