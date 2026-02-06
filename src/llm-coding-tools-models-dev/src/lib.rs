use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::{self, BufReader, BufWriter, Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::task;
use zstd::stream::decode_all;
use zstd::stream::write::Encoder;

const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";
const MODELS_DEV_API_URL_ENV: &str = "MODELS_DEV_API_URL";
const CACHE_PATH_ENV: &str = "OPENCODE_MODELS_DEV_CACHE_PATH";
static BUNDLED_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/models.dev.min.json.zst"));

/// Metadata for a models.dev provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub id: String,
    pub npm: Option<String>,
    pub api: Option<String>,
    pub env: Vec<String>,
}

/// Indicates where a catalog was loaded from.
#[derive(Debug, Clone)]
pub enum CatalogSource {
    Bundled,
    Cache(PathBuf),
    Downloaded(PathBuf),
}

/// Errors returned by the models.dev catalog.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("zstd error: {0}")]
    Zstd(std::io::Error),
    #[error("missing bundled snapshot")]
    MissingBundledSnapshot,
    #[error("task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

/// Outcome of loading from cache with bundled fallback.
pub struct CacheLoadResult {
    pub catalog: ModelsDevCatalog,
    pub source: CatalogSource,
    pub cache_error: Option<CatalogError>,
}

/// In-memory catalog with model→provider index.
#[derive(Debug, Clone)]
pub struct ModelsDevCatalog {
    providers: HashMap<String, ProviderMetadata>,
    models_to_providers: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    providers: HashMap<String, ProviderSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderSnapshot {
    id: String,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    models: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FullSnapshot {
    Nested {
        providers: HashMap<String, FullProvider>,
    },
    Flat(HashMap<String, FullProvider>),
}

impl FullSnapshot {
    fn into_providers(self) -> HashMap<String, FullProvider> {
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
    models: HashMap<String, ModelStub>,
}

#[derive(Debug, Deserialize)]
struct ModelStub {}

/// Resolve the shared cache path for models.dev snapshots.
///
/// Returns: `Some(PathBuf)` when a cache directory can be determined, or `None`.
pub fn shared_cache_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os(CACHE_PATH_ENV) {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    let base = if cfg!(target_os = "windows") {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library").join("Caches"))
    } else {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;

    Some(
        base.join("opencode")
            .join("models.dev")
            .join("models.dev.min.json.zst"),
    )
}

fn models_dev_api_url() -> String {
    env::var(MODELS_DEV_API_URL_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| MODELS_DEV_API_URL.to_string())
}

impl ModelsDevCatalog {
    /// Load the bundled snapshot embedded in the crate.
    ///
    /// Returns: the catalog and [`CatalogSource::Bundled`].
    pub fn from_bundled() -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_bundled_bytes(BUNDLED_ZST, None)?;
        Ok((catalog, CatalogSource::Bundled))
    }

    /// Load a cached snapshot from disk.
    ///
    /// Parameters:
    /// - `path`: zstd-compressed snapshot path.
    ///
    /// Returns: the catalog and [`CatalogSource::Cache`].
    pub fn from_cache(path: &Path) -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_compressed_path(path, None)?;
        Ok((catalog, CatalogSource::Cache(path.to_path_buf())))
    }

    /// Load a cached snapshot while keeping only selected model IDs.
    ///
    /// Parameters:
    /// - `path`: zstd-compressed snapshot path.
    /// - `model_ids`: model ID set to index.
    ///
    /// Returns: the filtered catalog and [`CatalogSource::Cache`].
    pub fn from_cache_filtered(
        path: &Path,
        model_ids: &HashSet<String>,
    ) -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_compressed_path(path, Some(model_ids))?;
        Ok((catalog, CatalogSource::Cache(path.to_path_buf())))
    }

    /// Load a downloaded snapshot from disk.
    ///
    /// Parameters:
    /// - `path`: zstd-compressed snapshot path.
    ///
    /// Returns: the catalog and [`CatalogSource::Downloaded`].
    pub fn from_downloaded(path: &Path) -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_compressed_path(path, None)?;
        Ok((catalog, CatalogSource::Downloaded(path.to_path_buf())))
    }

    /// Load a downloaded snapshot while keeping only selected model IDs.
    ///
    /// Parameters:
    /// - `path`: zstd-compressed snapshot path.
    /// - `model_ids`: model ID set to index.
    ///
    /// Returns: the filtered catalog and [`CatalogSource::Downloaded`].
    pub fn from_downloaded_filtered(
        path: &Path,
        model_ids: &HashSet<String>,
    ) -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_compressed_path(path, Some(model_ids))?;
        Ok((catalog, CatalogSource::Downloaded(path.to_path_buf())))
    }

    /// Load the bundled snapshot while keeping only selected model IDs.
    ///
    /// Parameters:
    /// - `model_ids`: model ID set to index.
    ///
    /// Returns: the filtered catalog and [`CatalogSource::Bundled`].
    pub fn from_bundled_filtered(
        model_ids: &HashSet<String>,
    ) -> Result<(Self, CatalogSource), CatalogError> {
        let catalog = Self::from_bundled_bytes(BUNDLED_ZST, Some(model_ids))?;
        Ok((catalog, CatalogSource::Bundled))
    }

    /// Load a catalog from a local models.dev `api.json` file.
    ///
    /// Parameters:
    /// - `path`: path to the full `api.json` file.
    ///
    /// Returns: a `ModelsDevCatalog` built from the minified schema.
    pub fn from_local_api_json(path: &Path) -> Result<Self, CatalogError> {
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        let snapshot = snapshot_from_full_reader(reader)?;
        Ok(Self::from_snapshot(snapshot, None))
    }

    /// Load from a cache path if available; fall back to bundled snapshot on missing/corrupt cache.
    ///
    /// Parameters:
    /// - `path`: zstd-compressed cache path to attempt.
    ///
    /// Returns: `CacheLoadResult` with `cache_error` set when fallback is due to corruption.
    pub fn load_cache_or_bundled(path: &Path) -> Result<CacheLoadResult, CatalogError> {
        match Self::from_cache(path) {
            Ok((catalog, source)) => Ok(CacheLoadResult {
                catalog,
                source,
                cache_error: None,
            }),
            Err(err) => {
                let cache_error = match &err {
                    CatalogError::Io(io_err) if io_err.kind() == io::ErrorKind::NotFound => None,
                    _ => Some(err),
                };
                let (catalog, source) = Self::from_bundled()?;
                Ok(CacheLoadResult {
                    catalog,
                    source,
                    cache_error,
                })
            }
        }
    }

    /// Primary entrypoint: load from shared cache if available; fall back to bundled snapshot.
    pub fn load_shared_cache_or_bundled() -> Result<CacheLoadResult, CatalogError> {
        if let Some(path) = shared_cache_path() {
            Self::load_cache_or_bundled(&path)
        } else {
            let (catalog, source) = Self::from_bundled()?;
            Ok(CacheLoadResult {
                catalog,
                source,
                cache_error: None,
            })
        }
    }

    /// Download the latest models.dev snapshot and write a compressed cache file.
    ///
    /// Parameters:
    /// - `path`: destination path for the zstd-compressed snapshot.
    ///
    /// Returns: `Ok(())` when the cache file is written.
    ///
    /// Honors the `MODELS_DEV_API_URL` environment variable when set and non-empty.
    ///
    /// This uses a two-pass I/O flow (download to disk, then read/strip/compress)
    /// to avoid buffering the full raw API response in memory (≈500KB–1MB).
    pub async fn download_to(path: &Path) -> Result<(), CatalogError> {
        let url = models_dev_api_url();
        Self::download_to_url(path, &url).await
    }

    /// Refresh a cache path by downloading and rewriting the snapshot.
    ///
    /// Parameters:
    /// - `path`: destination path for the zstd-compressed snapshot.
    ///
    /// Returns: `Ok(())` when the cache file is written.
    ///
    /// Honors the `MODELS_DEV_API_URL` environment variable when set and non-empty.
    ///
    /// This uses a two-pass I/O flow (download to disk, then read/strip/compress)
    /// to avoid buffering the full raw API response in memory (≈500KB–1MB).
    pub async fn refresh_cache(path: &Path) -> Result<(), CatalogError> {
        let url = models_dev_api_url();
        Self::download_to_url(path, &url).await
    }

    async fn download_to_url(path: &Path, url: &str) -> Result<(), CatalogError> {
        let tmp_download = path.with_extension("json.tmp");
        let tmp_cache = path.with_extension("json.zst.tmp");
        let result = Self::download_and_compress(url, &tmp_download, &tmp_cache).await;
        let result = match result {
            Ok(()) => match fs::rename(&tmp_cache, path).await {
                Ok(()) => Ok(()),
                Err(rename_err) => {
                    if cfg!(windows) && rename_err.kind() == io::ErrorKind::AlreadyExists {
                        let _ = fs::remove_file(path).await;
                        match fs::rename(&tmp_cache, path).await {
                            Ok(()) => Ok(()),
                            Err(_) => Err(rename_err.into()),
                        }
                    } else {
                        Err(rename_err.into())
                    }
                }
            },
            Err(err) => Err(err),
        };

        let _ = fs::remove_file(&tmp_download).await;
        if result.is_err() {
            let _ = fs::remove_file(&tmp_cache).await;
        }

        result
    }

    async fn download_and_compress(
        url: &str,
        tmp_download: &Path,
        tmp_cache: &Path,
    ) -> Result<(), CatalogError> {
        if let Some(parent) = tmp_download.parent() {
            fs::create_dir_all(parent).await?;
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(io::Error::other)?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(io::Error::other)?
            .error_for_status()
            .map_err(io::Error::other)?;

        let mut tmp_file = fs::File::create(tmp_download).await?;
        while let Some(chunk) = response.chunk().await.map_err(io::Error::other)? {
            tmp_file.write_all(&chunk).await?;
        }
        tmp_file.flush().await?;
        drop(tmp_file);

        let tmp_download = tmp_download.to_path_buf();
        let tmp_cache = tmp_cache.to_path_buf();
        task::spawn_blocking(move || {
            let raw = std::fs::File::open(&tmp_download)?;
            let reader = BufReader::new(raw);
            let snapshot = snapshot_from_full_reader(reader)?;

            if let Some(parent) = tmp_cache.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let file = std::fs::File::create(&tmp_cache)?;
            let writer = BufWriter::new(file);
            let mut encoder = Encoder::new(writer, 22).map_err(CatalogError::Zstd)?;
            serde_json::to_writer(&mut encoder, &snapshot)?;
            encoder.finish().map_err(CatalogError::Zstd)?;
            Ok::<(), CatalogError>(())
        })
        .await
        .map_err(CatalogError::JoinError)??;

        Ok(())
    }

    /// Resolve provider IDs for a model ID.
    ///
    /// Parameters:
    /// - `model_id`: models.dev model ID.
    ///
    /// Returns: `Some(&[String])` of provider IDs, or `None` if the model is unknown.
    #[inline]
    pub fn resolve_provider_for_model(&self, model_id: &str) -> Option<&[String]> {
        self.models_to_providers.get(model_id).map(Vec::as_slice)
    }

    /// Look up provider metadata by provider ID.
    ///
    /// Parameters:
    /// - `provider_id`: models.dev provider ID.
    ///
    /// Returns: provider metadata if present.
    #[inline]
    pub fn get_provider(&self, provider_id: &str) -> Option<&ProviderMetadata> {
        self.providers.get(provider_id)
    }

    fn from_snapshot_bytes(
        json: &[u8],
        model_filter: Option<&HashSet<String>>,
    ) -> Result<Self, CatalogError> {
        let snapshot: Snapshot = serde_json::from_slice(json)?;
        Ok(Self::from_snapshot(snapshot, model_filter))
    }

    fn from_compressed_path(
        path: &Path,
        model_filter: Option<&HashSet<String>>,
    ) -> Result<Self, CatalogError> {
        let compressed = std::fs::read(path)?;
        Self::from_compressed_bytes(&compressed, model_filter)
    }

    fn from_compressed_bytes(
        compressed: &[u8],
        model_filter: Option<&HashSet<String>>,
    ) -> Result<Self, CatalogError> {
        let json = decode_all(Cursor::new(compressed)).map_err(CatalogError::Zstd)?;
        Self::from_snapshot_bytes(&json, model_filter)
    }

    fn from_bundled_bytes(
        compressed: &[u8],
        model_filter: Option<&HashSet<String>>,
    ) -> Result<Self, CatalogError> {
        if compressed.is_empty() {
            return Err(CatalogError::MissingBundledSnapshot);
        }
        Self::from_compressed_bytes(compressed, model_filter)
    }

    fn from_snapshot(snapshot: Snapshot, model_filter: Option<&HashSet<String>>) -> Self {
        let mut providers = HashMap::with_capacity(snapshot.providers.len());
        let mut models_to_providers = HashMap::new();

        for (provider_id, provider) in snapshot.providers {
            let metadata = ProviderMetadata {
                id: provider.id.clone(),
                npm: provider.npm,
                api: provider.api,
                env: provider.env,
            };
            providers.insert(provider_id.clone(), metadata);

            for model_id in provider.models {
                if let Some(filter) = model_filter {
                    if !filter.contains(&model_id) {
                        continue;
                    }
                }
                models_to_providers
                    .entry(model_id)
                    .or_insert_with(Vec::new)
                    .push(provider_id.clone());
            }
        }

        Self {
            providers,
            models_to_providers,
        }
    }
}

fn snapshot_from_full_reader<R: Read>(reader: R) -> Result<Snapshot, CatalogError> {
    let full: FullSnapshot = serde_json::from_reader(reader)?;
    let full_providers = full.into_providers();
    let mut providers = HashMap::with_capacity(full_providers.len());
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
    Ok(Snapshot { providers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zstd::bulk::compress;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    fn snapshot_from_full_bytes(bytes: &[u8]) -> Result<Snapshot, CatalogError> {
        snapshot_from_full_reader(std::io::Cursor::new(bytes))
    }

    fn assert_cache_fallback(path: &Path, expect_error: bool) {
        let result = ModelsDevCatalog::load_cache_or_bundled(path).expect("fallback");
        assert!(matches!(result.source, CatalogSource::Bundled));
        if expect_error {
            assert!(result.cache_error.is_some());
        } else {
            assert!(result.cache_error.is_none());
        }
    }

    #[test]
    fn bundled_snapshot_loads() {
        let (catalog, source) = ModelsDevCatalog::from_bundled().expect("bundled snapshot loads");
        assert!(matches!(source, CatalogSource::Bundled));
        assert!(!catalog.providers.is_empty());
    }

    #[test]
    fn lookup_works_for_fixture_snapshot() {
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":["ALPHA_KEY"],"models":["m1","m2"]}}}"#;
        let catalog = ModelsDevCatalog::from_snapshot_bytes(json, None).expect("parse fixture");
        let providers = catalog.resolve_provider_for_model("m1").expect("providers");
        assert!(providers.iter().any(|id| id == "alpha"));
        let provider = catalog.get_provider("alpha").expect("provider exists");
        assert_eq!(provider.env, vec!["ALPHA_KEY".to_string()]);
    }

    #[test]
    fn from_cache_loads_fixture() {
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":["m1"]}}}"#;
        let compressed = compress(json, 22).expect("compress fixture");
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("snapshot.zst");
        std::fs::write(&path, compressed).expect("write cache");

        let (catalog, source) = ModelsDevCatalog::from_cache(&path).expect("cache loads");
        assert!(matches!(source, CatalogSource::Cache(_)));
        assert!(catalog.get_provider("alpha").is_some());
    }

    #[test]
    fn from_cache_filtered_keeps_selected_model() {
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":["m1","m2"]}}}"#;
        let compressed = compress(json, 22).expect("compress fixture");
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("snapshot.zst");
        std::fs::write(&path, compressed).expect("write cache");

        let mut filter = HashSet::new();
        filter.insert("m2".to_string());

        let (catalog, source) =
            ModelsDevCatalog::from_cache_filtered(&path, &filter).expect("cache filtered");
        assert!(matches!(source, CatalogSource::Cache(_)));
        assert!(catalog.resolve_provider_for_model("m1").is_none());
        let providers = catalog.resolve_provider_for_model("m2").expect("providers");
        assert!(providers.iter().any(|id| id == "alpha"));
    }

    #[test]
    fn from_bundled_filtered_keeps_selected_model() {
        let json = decode_all(Cursor::new(BUNDLED_ZST)).expect("decode bundled");
        let snapshot: Snapshot = serde_json::from_slice(&json).expect("parse bundled");
        let (model_id, provider_id) = snapshot
            .providers
            .values()
            .find_map(|provider| {
                provider
                    .models
                    .first()
                    .map(|id| (id.clone(), provider.id.clone()))
            })
            .expect("bundled has model");
        let mut filter = HashSet::new();
        filter.insert(model_id.clone());

        let (catalog, source) =
            ModelsDevCatalog::from_bundled_filtered(&filter).expect("filtered load");
        assert!(matches!(source, CatalogSource::Bundled));
        let providers = catalog
            .resolve_provider_for_model(&model_id)
            .expect("providers");
        assert!(providers.iter().any(|id| id == &provider_id));
    }

    #[test]
    fn missing_bundled_snapshot_errors() {
        let err = ModelsDevCatalog::from_bundled_bytes(&[], None).expect_err("missing bundled");
        assert!(matches!(err, CatalogError::MissingBundledSnapshot));
    }

    #[test]
    fn corrupt_zstd_errors() {
        let err =
            ModelsDevCatalog::from_compressed_bytes(b"not-zstd", None).expect_err("corrupt zstd");
        assert!(matches!(err, CatalogError::Zstd(_)));
    }

    #[test]
    fn json_parse_errors() {
        let err = ModelsDevCatalog::from_snapshot_bytes(b"{not json}", None).expect_err("bad json");
        assert!(matches!(err, CatalogError::Json(_)));
    }

    #[test]
    fn lookup_cases_parameterized() {
        let json = br#"{"providers":{
            "alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":["m1","m2"]},
            "beta":{"id":"beta","npm":null,"api":null,"env":[],"models":["m2"]}
        }}"#;
        let catalog = ModelsDevCatalog::from_snapshot_bytes(json, None).expect("parse fixture");

        let cases = [
            ("m2", &["alpha", "beta"][..]),
            ("m1", &["alpha"][..]),
            ("missing", &[][..]),
        ];
        for (model_id, expected) in cases {
            let providers = catalog.resolve_provider_for_model(model_id).unwrap_or(&[]);
            let mut providers = providers.iter().map(String::as_str).collect::<Vec<_>>();
            providers.sort_unstable();
            let mut expected = expected.to_vec();
            expected.sort_unstable();
            assert_eq!(providers, expected);
        }

        assert!(catalog.get_provider("missing").is_none());
    }

    #[test]
    fn snapshot_from_full_bytes_accepts_flat_map() {
        let json = br#"{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}"#;
        let snapshot = snapshot_from_full_bytes(json).expect("parse flat full snapshot");
        let provider = snapshot.providers.get("alpha").expect("alpha provider");
        assert_eq!(provider.models, vec!["m1".to_string()]);
    }

    #[test]
    fn snapshot_from_full_bytes_accepts_nested_map() {
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        let snapshot = snapshot_from_full_bytes(json).expect("parse nested full snapshot");
        let provider = snapshot.providers.get("alpha").expect("alpha provider");
        assert_eq!(provider.models, vec!["m1".to_string()]);
    }

    #[tokio::test]
    async fn download_to_writes_snapshot() {
        let server = MockServer::start().await;
        let body = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("download.zst");
        ModelsDevCatalog::download_to_url(&path, &format!("{}/api.json", server.uri()))
            .await
            .expect("download");
        let (catalog, source) = ModelsDevCatalog::from_downloaded(&path).expect("load downloaded");
        assert!(matches!(source, CatalogSource::Downloaded(_)));
        assert!(catalog.get_provider("alpha").is_some());

        let mut filter = HashSet::new();
        filter.insert("m1".to_string());
        let (filtered, source) = ModelsDevCatalog::from_downloaded_filtered(&path, &filter)
            .expect("load downloaded filtered");
        assert!(matches!(source, CatalogSource::Downloaded(_)));
        assert!(filtered.resolve_provider_for_model("m1").is_some());
        assert!(filtered.resolve_provider_for_model("missing").is_none());
    }

    #[tokio::test]
    async fn download_to_creates_parent_directories() {
        let server = MockServer::start().await;
        let body = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("nested/dir/download.zst");
        ModelsDevCatalog::download_to_url(&path, &format!("{}/api.json", server.uri()))
            .await
            .expect("download");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn download_to_errors_on_bad_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("download.zst");
        let err = ModelsDevCatalog::download_to_url(&path, &format!("{}/api.json", server.uri()))
            .await
            .expect_err("bad status");
        assert!(matches!(err, CatalogError::Io(_)));
    }

    #[tokio::test]
    async fn refresh_cache_writes_valid_snapshot_and_cleans_temp() {
        let server = MockServer::start().await;
        let body = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("cache/models.dev.min.json.zst");
        ModelsDevCatalog::download_to_url(&path, &format!("{}/api.json", server.uri()))
            .await
            .expect("refresh");

        let (catalog, source) = ModelsDevCatalog::from_cache(&path).expect("load cache");
        assert!(matches!(source, CatalogSource::Cache(_)));
        assert!(catalog.get_provider("alpha").is_some());

        let tmp_download = path.with_extension("json.tmp");
        let tmp_cache = path.with_extension("json.zst.tmp");
        assert!(!tmp_download.exists());
        assert!(!tmp_cache.exists());
    }

    #[test]
    fn from_local_api_json_loads_minified_schema() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("api.json");
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":["ALPHA_KEY"],"models":{"m1":{}}}}}"#;
        std::fs::write(&path, json).expect("write api.json");
        let catalog = ModelsDevCatalog::from_local_api_json(&path).expect("load local");
        assert!(catalog.get_provider("alpha").is_some());
        assert!(catalog.resolve_provider_for_model("m1").is_some());
    }

    #[test]
    fn cache_error_is_none_for_missing_cache() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing.zst");
        assert_cache_fallback(&path, false);
    }

    #[test]
    fn cache_error_is_some_for_corrupt_cache() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("bad.zst");
        std::fs::write(&path, b"not-zstd").expect("write bad zstd");
        assert_cache_fallback(&path, true);
    }

    #[test]
    fn snapshot_strips_model_fields_to_string_list() {
        let json = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{"description":"desc"}}}}}"#;
        let snapshot = snapshot_from_full_bytes(json).expect("snapshot");
        let provider = snapshot.providers.get("alpha").expect("provider");
        assert_eq!(provider.models, vec!["m1".to_string()]);
    }

    #[tokio::test]
    async fn download_uses_env_override_url() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        let body = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        std::env::set_var(MODELS_DEV_API_URL_ENV, format!("{}/api.json", server.uri()));
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("download.zst");
        ModelsDevCatalog::download_to(&path)
            .await
            .expect("download");
        std::env::remove_var(MODELS_DEV_API_URL_ENV);
    }

    async fn assert_shared_cache_fallback(path: &Path, corrupt_payload: Option<&[u8]>) {
        let _guard = ENV_LOCK.lock().await;
        if let Some(payload) = corrupt_payload {
            std::fs::write(path, payload).expect("write cache");
        }
        std::env::set_var(CACHE_PATH_ENV, path);
        let result = ModelsDevCatalog::load_shared_cache_or_bundled().expect("fallback");
        std::env::remove_var(CACHE_PATH_ENV);
        assert!(matches!(result.source, CatalogSource::Bundled));
        assert_eq!(result.cache_error.is_some(), corrupt_payload.is_some());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn refresh_cache_windows_already_exists_fallback() {
        let server = MockServer::start().await;
        let body = br#"{"providers":{"alpha":{"id":"alpha","npm":null,"api":null,"env":[],"models":{"m1":{}}}}}"#;
        Mock::given(method("GET"))
            .and(path("/api.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("cache/models.dev.min.json.zst");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.expect("mkdir");
        }
        fs::write(&path, b"existing").await.expect("write existing");

        ModelsDevCatalog::download_to_url(&path, &format!("{}/api.json", server.uri()))
            .await
            .expect("refresh with fallback");

        let (catalog, source) = ModelsDevCatalog::from_cache(&path).expect("load cache");
        assert!(matches!(source, CatalogSource::Cache(_)));
        assert!(catalog.get_provider("alpha").is_some());
    }

    #[tokio::test]
    async fn load_shared_cache_fallback_variants() {
        let temp = TempDir::new().expect("tempdir");
        let missing = temp.path().join("missing.zst");
        let corrupt = temp.path().join("bad.zst");
        assert_shared_cache_fallback(&missing, None).await;
        assert_shared_cache_fallback(&corrupt, Some(b"not-zstd")).await;
    }
}
