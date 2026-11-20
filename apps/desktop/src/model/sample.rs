//! Dossiers proposés au premier lancement (avant toute sauvegarde locale).

use std::path::PathBuf;

use super::Project;

pub fn projects() -> Vec<Project> {
    let home = crate::paths::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let async_dir = home.join("Developpement/Async");
    [
        async_dir.join("demo-repo"),
        async_dir.join("AsyncRustAPI"),
        async_dir.join("sample-node-app"),
        home.join("Developpement/sup2flow"),
    ]
    .into_iter()
    .filter(|path| path.is_dir())
    .map(Project::from_path)
    .collect()
}
