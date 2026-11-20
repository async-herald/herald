use std::path::PathBuf;

use gpui::SharedString;
use serde::{Deserialize, Serialize};

use super::Severity;

/// Vulnérabilité publiée (base OSV) qui touche une dépendance d'un projet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Advisory {
    /// Identifiant affiché : le CVE s'il existe, sinon l'identifiant OSV (GHSA…).
    pub id: SharedString,
    /// Identifiant OSV, clé de l'alerte.
    pub osv_id: SharedString,
    pub summary: SharedString,
    pub severity: Severity,
    /// Score CVSS sur 10 ; `None` si l'avis n'en publie pas.
    pub cvss: Option<f32>,
    /// Écosystème du paquet (npm, crates.io, PyPI, Go).
    pub ecosystem: SharedString,
    pub package: SharedString,
    /// Version déclarée dans le projet.
    pub installed_version: SharedString,
    /// Première version corrigée, si elle existe.
    pub fixed_version: Option<SharedString>,
    /// Nom du projet concerné.
    pub project: SharedString,
    /// Dossier du projet concerné (clé du projet).
    pub project_path: PathBuf,
    /// Page de l'avis (GitHub Advisory ou osv.dev).
    pub url: SharedString,
    /// Date de publication en secondes Unix (0 : inconnue).
    pub published_unix: u64,
    pub read: bool,
    /// Version installée connue (lockfile ou version figée). Sinon la version
    /// est la borne basse d'une contrainte du manifeste : la faille est à
    /// confirmer, la version installée pouvant déjà être corrigée.
    #[serde(default = "yes")]
    pub confirmed: bool,
    /// Dépendance déclarée par le projet (sinon : transitive).
    #[serde(default = "yes")]
    pub direct: bool,
}

fn yes() -> bool {
    true
}

impl Advisory {
    /// « express 4.18.2 → 4.21.1 » ou « express 4.18.2 (pas de correctif) ».
    pub fn upgrade_label(&self) -> String {
        match &self.fixed_version {
            Some(fixed) => format!("{} {} → {fixed}", self.package, self.installed_version),
            None => format!(
                "{} {} (pas encore de correctif)",
                self.package, self.installed_version
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn advisory_for_tests(fixed: Option<&str>) -> Advisory {
    Advisory {
        id: "CVE-2024-0001".into(),
        osv_id: "GHSA-xxxx-yyyy-zzzz".into(),
        summary: "Exemple".into(),
        severity: Severity::Critical,
        cvss: Some(9.8),
        ecosystem: "npm".into(),
        package: "express".into(),
        installed_version: "4.18.2".into(),
        fixed_version: fixed.map(|f| SharedString::from(f.to_owned())),
        project: "demo".into(),
        project_path: PathBuf::from("/tmp/demo"),
        url: "https://osv.dev/vulnerability/GHSA-xxxx-yyyy-zzzz".into(),
        published_unix: 0,
        read: false,
        confirmed: true,
        direct: true,
    }
}

#[cfg(test)]
mod tests {
    use super::advisory_for_tests;

    #[test]
    fn upgrade_label_mentions_fixed_version() {
        assert!(
            advisory_for_tests(Some("4.21.1"))
                .upgrade_label()
                .contains('→')
        );
        assert!(
            advisory_for_tests(None)
                .upgrade_label()
                .contains("pas encore")
        );
    }
}
