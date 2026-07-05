use serde::Deserialize;

#[derive(Deserialize)]
struct CratesResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(Deserialize)]
struct CrateInfo {
    max_stable_version: String,
}

pub(super) async fn latest(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let body: CratesResponse = client.get(url).send().await.ok()?.json().await.ok()?;
    Some(body.krate.max_stable_version)
}
