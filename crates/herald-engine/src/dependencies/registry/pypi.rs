use serde::Deserialize;

#[derive(Deserialize)]
struct PypiResponse {
    info: PypiInfo,
}

#[derive(Deserialize)]
struct PypiInfo {
    version: String,
}

pub(super) async fn latest(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{name}/json");
    let body: PypiResponse = client.get(url).send().await.ok()?.json().await.ok()?;
    Some(body.info.version)
}
