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

#[derive(Debug, Clone, Serialize)]
pub enum Grade {
    A, B, C, D, F,
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
    #[serde(default)]
    pub commit: Option<String>,
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