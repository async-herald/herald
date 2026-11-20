//! Actions sur le code depuis la fiche d'un problème : le corriger dans l'IDE
//! intégré (vue « Code ») ou ouvrir le fichier dans VS Code.

use std::process::Command;

use gpui::{Window, prelude::*};

use super::app::HeraldApp;

impl HeraldApp {
    /// Ouvre le fichier de l'occurrence affichée dans l'IDE, à sa ligne.
    pub(super) fn fix_in_ide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_issue_index() else {
            return;
        };
        let Some((file, line)) = self
            .report()
            .and_then(|report| report.issues.get(ix))
            .map(|issue| (issue.file.to_string(), issue.line))
        else {
            return;
        };
        self.open_in_ide(&file, line, window, cx);
    }

    /// Ouvre le fichier à la ligne du problème dans VS Code, sinon dans l'app par défaut.
    pub(super) fn open_in_editor(&mut self, cx: &mut Context<Self>) {
        let (Some(project), Some(ix)) = (self.project(), self.selected_issue_index()) else {
            return;
        };
        let Some(issue) = project.report.as_ref().and_then(|r| r.issues.get(ix)) else {
            return;
        };
        let path = project.path.join(issue.file.as_ref());
        let line = issue.line.unwrap_or(1);
        cx.background_executor()
            .spawn(async move {
                let target = format!("{}:{line}", path.display());
                let opened_in_code = Command::new("code")
                    .args(["--goto", &target])
                    .status()
                    .is_ok_and(|status| status.success());
                if !opened_in_code {
                    let _ = Command::new("open").arg(&path).status();
                }
            })
            .detach();
    }
}
