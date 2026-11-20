use serde::{Deserialize, Serialize};

use tree_sitter::Tree;

use crate::{category::Category, language::Language};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Deserialize)]
pub struct FileInput {
    pub path: String,
    pub code: String,
}

/// Note à 13 paliers (A+ à F) plutôt que 5 lettres brutes — un score qui
/// stagne à 92 restait invisible tant qu'il ne franchissait pas 90 puis 100 ;
/// les +/- rendent la progression lisible dans cet intervalle. `F` reste sans
/// subdivision : en dessous de 40, l'écart entre les repos ne mérite pas plus
/// de granularité que "à revoir en profondeur".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Grade {
    #[serde(rename = "A+")]
    APlus,
    A,
    #[serde(rename = "A-")]
    AMinus,
    #[serde(rename = "B+")]
    BPlus,
    B,
    #[serde(rename = "B-")]
    BMinus,
    #[serde(rename = "C+")]
    CPlus,
    C,
    #[serde(rename = "C-")]
    CMinus,
    #[serde(rename = "D+")]
    DPlus,
    D,
    #[serde(rename = "D-")]
    DMinus,
    F,
}

impl Grade {
    pub const fn label(self) -> &'static str {
        match self {
            Self::APlus => "A+",
            Self::A => "A",
            Self::AMinus => "A-",
            Self::BPlus => "B+",
            Self::B => "B",
            Self::BMinus => "B-",
            Self::CPlus => "C+",
            Self::C => "C",
            Self::CMinus => "C-",
            Self::DPlus => "D+",
            Self::D => "D",
            Self::DMinus => "D-",
            Self::F => "F",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Score {
    pub value: u32,
    pub grade: Grade,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub files: Vec<FileInput>,
    /// Optional git context — enables `branch:`/`commit:` rules.
    #[serde(default)]
    pub git: Option<GitContext>,
}

#[derive(Debug, Default, Deserialize)]
pub struct GitContext {
    #[serde(default)]
    pub branch: Option<String>,
    /// Sujets des derniers commits (les plus récents d'abord). Chacun est
    /// évalué contre les règles `commit:` ; les correspondances sont
    /// dédupliquées par règle (voir `analyze()`) pour qu'un dépôt à
    /// l'historique répétitif (ex. 20 commits "wip") ne pénalise pas plus
    /// qu'un dépôt avec un seul commit fautif.
    #[serde(default)]
    pub commits: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FileResult {
    pub path: String,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub files: Vec<FileResult>,
    pub total_issues: usize,
    pub score: Score,
}

pub struct FnRule {
    pub name: &'static str,
    pub category: Category,
    pub check: fn(&str, Option<&Tree>, Language) -> Vec<Issue>,
}

pub trait Rule: Sync {
    fn name(&self) -> &'static str;
    fn category(&self) -> &Category;
    fn run(&self, code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue>;
}

impl Rule for FnRule {
    fn name(&self) -> &'static str {
        self.name
    }
    fn category(&self) -> &Category {
        &self.category
    }
    fn run(&self, code: &str, tree: Option<&Tree>, language: Language) -> Vec<Issue> {
        (self.check)(code, tree, language)
    }
}