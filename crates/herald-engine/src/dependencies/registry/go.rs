use serde::Deserialize;

#[derive(Deserialize)]
struct GoLatest {
    #[serde(rename = "Version")]
    version: String,
}

pub(super) async fn latest(client: &reqwest::Client, name: &str) -> Option<String> {
    let url = format!("https://proxy.golang.org/{name}/@latest");
    let body: GoLatest = client.get(url).send().await.ok()?.json().await.ok()?;
    Some(body.version.trim_start_matches('v').to_string())
}
