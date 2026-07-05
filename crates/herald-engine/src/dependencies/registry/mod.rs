mod cargo;
mod go;
mod npm;
mod pypi;

use crate::dependencies::PackageManager;

pub async fn latest(
    client: &reqwest::Client,
    manager: PackageManager,
    name: &str,
) -> Option<String> {
    match manager {
        PackageManager::Npm => npm::latest(client, name).await,
        PackageManager::Cargo => cargo::latest(client, name).await,
        PackageManager::Pip => pypi::latest(client, name).await,
        PackageManager::Go => go::latest(client, name).await,
    }
}
