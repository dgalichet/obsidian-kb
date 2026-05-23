use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::KbError;
use crate::models::SearchMode;

pub const CONFIG_FILE_NAME: &str = ".obsidian-kb.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub vault: VaultConfig,
    pub index: IndexConfig,
    pub search: SearchConfig,
    pub embeddings: EmbeddingConfig,
    #[serde(default)]
    pub benchmark: BenchmarkConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub serve: ServeConfig,
    #[serde(default)]
    pub doctor: DoctorConfig,
    #[serde(skip)]
    pub config_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    pub path: PathBuf,
    pub exclude_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    pub store_dir: PathBuf,
    pub database_path: PathBuf,
    pub tantivy_index_dir: PathBuf,
    pub chunk_target_chars: usize,
    pub chunk_overlap_chars: usize,
    pub max_chunk_chars: usize,
    #[serde(default)]
    pub exclude_headings: Vec<String>,
    pub remove_diacritics: bool,
    #[serde(default)]
    pub properties: PropertyIndexConfig,
    #[serde(default)]
    pub pdf: PdfIndexConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_mode: String,
    pub bm25_candidates: usize,
    pub vector_candidates: usize,
    pub final_top_k: usize,
    pub rrf_k: f32,
    pub bm25_weight: f32,
    pub vector_weight: f32,
    pub graph_weight: f32,
    pub graph_depth: usize,
    pub graph_max_neighbors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyIndexConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_property_filter_keys")]
    pub filter_keys: Vec<String>,
    #[serde(default = "default_property_ignored_keys")]
    pub ignored_keys: Vec<String>,
    #[serde(default = "default_property_max_value_chars")]
    pub max_value_chars: usize,
}

impl Default for PropertyIndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            filter_keys: default_property_filter_keys(),
            ignored_keys: default_property_ignored_keys(),
            max_value_chars: default_property_max_value_chars(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfIndexConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_pdf_max_file_size_mb")]
    pub max_file_size_mb: usize,
}

impl Default for PdfIndexConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_file_size_mb: default_pdf_max_file_size_mb(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub batch_size: usize,
    pub normalize: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub enabled: bool,
    pub log_path: PathBuf,
    pub include_query: bool,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_path: PathBuf::from(".obsidian-kb/benchmarks.jsonl"),
            include_query: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub idle_unload_seconds: u64,
    pub preload_embedder: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            idle_unload_seconds: 600,
            preload_embedder: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeConfig {
    #[serde(default = "default_cors_allowed_origins")]
    pub cors_allowed_origins: Vec<String>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            cors_allowed_origins: default_cors_allowed_origins(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DoctorConfig {
    #[serde(default)]
    pub unresolved_links: DoctorUnresolvedLinksConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DoctorUnresolvedLinksConfig {
    #[serde(default)]
    pub allow_forward_links: bool,
    #[serde(default)]
    pub ignore_targets: Vec<String>,
    #[serde(default)]
    pub ignore_globs: Vec<String>,
}

impl AppConfig {
    pub fn default_for_vault(vault: &Path, store_dir: Option<&Path>) -> Result<Self> {
        Self::default_for_vault_in(vault, store_dir, &std::env::current_dir()?)
    }

    pub fn default_for_vault_in(
        vault: &Path,
        store_dir: Option<&Path>,
        config_dir: &Path,
    ) -> Result<Self> {
        let vault_path = std::fs::canonicalize(vault)
            .with_context(|| format!("vault does not exist: {}", vault.display()))?;
        if !vault_path.is_dir() {
            anyhow::bail!("vault is not a directory: {}", vault_path.display());
        }

        let store_dir = store_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".obsidian-kb"));
        Ok(Self {
            vault: VaultConfig {
                path: vault_path,
                exclude_globs: vec![
                    ".obsidian/**".to_string(),
                    ".obsidian-kb/**".to_string(),
                    ".trash/**".to_string(),
                    "Templates/**".to_string(),
                    "**/*.excalidraw.md".to_string(),
                ],
            },
            index: IndexConfig {
                database_path: store_dir.join("metadata.sqlite"),
                tantivy_index_dir: store_dir.join("tantivy"),
                store_dir,
                chunk_target_chars: 3000,
                chunk_overlap_chars: 300,
                max_chunk_chars: 5000,
                exclude_headings: Vec::new(),
                remove_diacritics: true,
                properties: PropertyIndexConfig::default(),
                pdf: PdfIndexConfig::default(),
            },
            search: SearchConfig {
                default_mode: "hybrid".to_string(),
                bm25_candidates: 80,
                vector_candidates: 80,
                final_top_k: 10,
                rrf_k: 60.0,
                bm25_weight: 1.0,
                vector_weight: 1.0,
                graph_weight: 0.25,
                graph_depth: 1,
                graph_max_neighbors: 20,
            },
            embeddings: EmbeddingConfig {
                enabled: true,
                provider: "fastembed".to_string(),
                model: "MultilingualE5Small".to_string(),
                batch_size: 64,
                normalize: true,
                cache_dir: None,
            },
            benchmark: BenchmarkConfig::default(),
            mcp: McpConfig::default(),
            serve: ServeConfig::default(),
            doctor: DoctorConfig::default(),
            config_dir: config_dir.to_path_buf(),
        })
    }

    pub fn vault_path(&self) -> &Path {
        &self.vault.path
    }

    pub fn store_dir(&self) -> PathBuf {
        self.resolve_vault_relative(&self.index.store_dir)
    }

    pub fn database_path(&self) -> PathBuf {
        self.resolve_vault_relative(&self.index.database_path)
    }

    pub fn tantivy_index_dir(&self) -> PathBuf {
        self.resolve_vault_relative(&self.index.tantivy_index_dir)
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    pub fn default_search_mode(&self) -> SearchMode {
        SearchMode::from_config_value(&self.search.default_mode).unwrap_or(SearchMode::Hybrid)
    }

    pub fn embedding_dimensions(&self) -> usize {
        crate::embeddings::embedding_dimension(&self.embeddings.model).unwrap_or(384)
    }

    pub fn embedding_model_key(&self) -> String {
        format!("{}:{}", self.embeddings.provider, self.embeddings.model)
    }

    /// Returns the directory where FastEmbed model files should be cached.
    ///
    /// The default is shared across vaults through the user's cache directory.
    pub fn embedding_cache_dir(&self) -> PathBuf {
        self.embeddings
            .cache_dir
            .as_deref()
            .map(|path| self.resolve_user_config_path(path))
            .unwrap_or_else(default_embedding_cache_dir)
    }

    pub fn embedding_min_cosine(&self) -> f32 {
        0.05
    }

    /// Returns the benchmark JSONL log path resolved relative to the config file.
    pub fn benchmark_log_path(&self) -> PathBuf {
        self.resolve_user_config_path(&self.benchmark.log_path)
    }

    fn resolve_vault_relative(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config_dir.join(path)
        }
    }

    fn resolve_user_config_path(&self, path: &Path) -> PathBuf {
        if let Some(expanded) = expand_tilde(path) {
            expanded
        } else if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config_dir.join(path)
        }
    }
}

fn default_embedding_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("obsidian-kb")
        .join("models")
}

fn default_true() -> bool {
    true
}

fn default_property_filter_keys() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_property_ignored_keys() -> Vec<String> {
    ["cssclasses", "template", "id", "uuid", "publish", "dg-*"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

fn default_property_max_value_chars() -> usize {
    200
}

fn default_pdf_max_file_size_mb() -> usize {
    50
}

fn default_cors_allowed_origins() -> Vec<String> {
    vec!["app://obsidian.md".to_string()]
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    std::fs::create_dir_all(config.store_dir())?;
    let path = config_path(config);
    let toml = toml::to_string_pretty(config)?;
    std::fs::write(path, toml)?;
    Ok(())
}

pub fn load_existing(vault: Option<&Path>, config: Option<&Path>) -> Result<AppConfig> {
    let path = resolve_config_path(vault, config)?;
    if !path.exists() {
        return Err(KbError::MissingConfig.into());
    }
    load_config_file(&path)
}

pub fn load_or_default(vault: Option<&Path>, config: Option<&Path>) -> Result<AppConfig> {
    if let Ok(path) = resolve_config_path(vault, config)
        && path.exists()
    {
        return load_config_file(&path);
    }
    let vault = vault.unwrap_or(Path::new("."));
    let config_dir = if vault.is_absolute() || vault.exists() {
        std::fs::canonicalize(vault)?
    } else {
        std::env::current_dir()?
    };
    AppConfig::default_for_vault_in(vault, None, &config_dir)
}

pub fn config_path(config: &AppConfig) -> PathBuf {
    config.config_path()
}

fn resolve_config_path(vault: Option<&Path>, config: Option<&Path>) -> Result<PathBuf> {
    if let Some(config) = config {
        return absolutize(config);
    }
    if let Some(vault) = vault {
        return Ok(std::fs::canonicalize(vault)?.join(CONFIG_FILE_NAME));
    }
    let candidate = std::env::current_dir()?.join(CONFIG_FILE_NAME);
    if candidate.exists() {
        return Ok(candidate);
    }
    Err(KbError::MissingConfig.into())
}

fn load_config_file(path: &Path) -> Result<AppConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config: {}", path.display()))?;
    let mut config: AppConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse config: {}", path.display()))?;
    let config_dir = path.parent().unwrap_or(Path::new("."));
    config.vault.path = absolutize_from(config_dir, &config.vault.path)?;
    config.config_dir = std::fs::canonicalize(config_dir)?;
    Ok(config)
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn absolutize_from(base: &Path, path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(base.join(path))
    }
}

fn expand_tilde(path: &Path) -> Option<PathBuf> {
    let value = path.to_str()?;
    if value == "~" {
        return dirs::home_dir();
    }
    value
        .strip_prefix("~/")
        .and_then(|suffix| dirs::home_dir().map(|home| home.join(suffix)))
}
