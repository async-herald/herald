use std::path::{Path, PathBuf};

use gpui::SharedString;

use super::{AnalysisReport, Grade};

/// Un dossier analysé (ou à analyser) par l'utilisateur.
#[derive(Clone, Debug)]
pub struct Project {
    pub name: SharedString,
    pub path: PathBuf,
    /// Adresse du dépôt distant d'où le dossier a été cloné (dépôt Git, démo).
    /// `None` pour un dossier local de l'utilisateur.
    pub remote: Option<String>,
    /// Dernier rapport ; `None` tant que le projet n'a jamais été analysé.
    pub report: Option<AnalysisReport>,
}

impl Project {
    /// Crée un projet non analysé à partir d'un dossier choisi par l'utilisateur.
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Self {
            name: name.into(),
            path,
            remote: None,
            report: None,
        }
    }

    /// Projet cloné depuis un dépôt distant dans `path`.
    pub fn cloned(path: PathBuf, remote: String) -> Self {
        Self {
            remote: Some(remote),
            ..Self::from_path(path)
        }
    }

    pub fn grade(&self) -> Option<Grade> {
        self.report.as_ref().map(|report| report.summary.grade())
    }

    /// Chemin affiché, avec `~` à la place du dossier personnel.
    pub fn display_path(&self) -> SharedString {
        shorten_home(&self.path).into()
    }
}

fn shorten_home(path: &Path) -> String {
    match crate::paths::home_dir() {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_uses_folder_name() {
        let project = Project::from_path(PathBuf::from("/tmp/mon-projet"));
        assert_eq!(project.name.as_ref(), "mon-projet");
        assert!(project.grade().is_none());
    }
}
