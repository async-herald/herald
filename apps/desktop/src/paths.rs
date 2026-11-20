//! Dossiers de l'app selon le système.
//!
//! | Système | Données de l'app |
//! |---|---|
//! | macOS   | `~/Library/Application Support/fr.itsasync.herald.desktop` (inchangé) |
//! | Windows | `%APPDATA%\fr.itsasync.herald.desktop` |
//! | Linux   | `~/.local/share/fr.itsasync.herald.desktop` (ou `$XDG_DATA_HOME`) |

use std::path::PathBuf;

/// Identifiant de l'app : nom du dossier de données, du Trousseau, du bundle.
pub const APP_ID: &str = "fr.itsasync.herald.desktop";

/// Dossier des données de l'app (base, dépôts clonés, session de développement).
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_ID))
}

/// Dossier personnel de l'utilisateur (`~`, `C:\Users\<nom>`).
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sur macOS, le dossier ne change pas : les projets, analyses et alertes
    /// déjà enregistrés restent là où ils sont.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_data_dir_is_unchanged() {
        let home = home_dir().unwrap();
        assert_eq!(
            data_dir().unwrap(),
            home.join("Library/Application Support/fr.itsasync.herald.desktop")
        );
    }
}
