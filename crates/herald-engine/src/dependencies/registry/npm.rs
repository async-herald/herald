use serde::Deserialize;

#[derive(Deserialize)]
struct NpmLatest {
    version: String,
}

pub(super) async fn latest(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://registry.npmjs.org/{name}/latest");
    let body: NpmLatest = client.get(url).send().await.ok()?.json().await.ok()?;
    Some(body.version)
}
