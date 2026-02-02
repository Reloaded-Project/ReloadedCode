use reqwest::Client;
use serde::Deserialize;
use serde_json::to_vec_pretty;
use std::{collections::BTreeMap, env, path::PathBuf, time::Duration};
use tokio::fs;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FullSnapshot {
    Nested {
        providers: std::collections::HashMap<String, FullProvider>,
    },
    Flat(std::collections::HashMap<String, FullProvider>),
}

impl FullSnapshot {
    fn into_providers(self) -> std::collections::HashMap<String, FullProvider> {
        match self {
            FullSnapshot::Nested { providers } => providers,
            FullSnapshot::Flat(providers) => providers,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FullProvider {
    id: String,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    models: std::collections::HashMap<String, ModelStub>,
}

#[derive(Debug, Deserialize)]
struct ModelStub {}

#[derive(Debug, serde::Serialize)]
struct Snapshot {
    providers: BTreeMap<String, ProviderSnapshot>,
}

#[derive(Debug, serde::Serialize)]
struct ProviderSnapshot {
    id: String,
    npm: Option<String>,
    api: Option<String>,
    env: Vec<String>,
    models: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = manifest_dir.join("data/models.dev.min.json");

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client
        .get("https://models.dev/api.json")
        .send()
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    let full: FullSnapshot = serde_json::from_slice(&bytes)?;
    let full_providers = full.into_providers();
    let mut providers = BTreeMap::new();
    for (provider_id, provider) in full_providers {
        let mut models = provider.models.into_keys().collect::<Vec<_>>();
        models.sort();
        providers.insert(
            provider_id,
            ProviderSnapshot {
                id: provider.id,
                npm: provider.npm,
                api: provider.api,
                env: provider.env,
                models,
            },
        );
    }

    let snapshot = Snapshot { providers };
    let json = to_vec_pretty(&snapshot)?;
    fs::write(output, json).await?;
    Ok(())
}
