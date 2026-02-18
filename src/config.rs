use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main configuration for Bobbin
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub index: IndexConfig,
    pub embedding: EmbeddingConfig,
    pub search: SearchConfig,
    pub git: GitConfig,
    pub dependencies: DependencyConfig,
    pub hooks: HooksConfig,
    pub beads: BeadsConfig,
    pub archive: ArchiveConfig,
}

/// Configuration for indexing behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    /// Glob patterns to include
    pub include: Vec<String>,
    /// Glob patterns to exclude (in addition to .gitignore)
    pub exclude: Vec<String>,
    /// Whether to respect .gitignore
    pub use_gitignore: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            include: vec![
                "**/*.rs".into(),
                "**/*.ts".into(),
                "**/*.tsx".into(),
                "**/*.js".into(),
                "**/*.jsx".into(),
                "**/*.py".into(),
                "**/*.go".into(),
                "**/*.md".into(),
            ],
            exclude: vec![
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/dist/**".into(),
                "**/.git/**".into(),
                "**/build/**".into(),
                "**/__pycache__/**".into(),
            ],
            use_gitignore: true,
        }
    }
}

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    /// Embedding backend: "onnx" (default) or "openai-api"
    pub backend: EmbeddingBackend,
    /// Model name (built-in ONNX model name or API model identifier)
    pub model: String,
    /// Batch size for embedding generation
    pub batch_size: usize,
    /// Embedding dimensions (auto-detected for built-in ONNX models, required for custom/API)
    pub dimensions: Option<usize>,
    /// Enable GPU acceleration for ONNX inference (CUDA).
    /// Also controllable via BOBBIN_GPU=1 env var.
    pub gpu: bool,
    /// OpenAI-compatible API settings (required when backend = "openai-api")
    pub api: Option<ApiEmbeddingConfig>,
    /// Custom local ONNX model settings (optional, for non-built-in models)
    pub custom_model: Option<CustomModelConfig>,
    /// Contextual embedding settings
    pub context: ContextualEmbeddingConfig,
}

/// Embedding backend type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingBackend {
    /// Local ONNX runtime inference
    Onnx,
    /// OpenAI-compatible embedding API (works with Ollama, vLLM, LiteLLM, etc.)
    OpenaiApi,
}

impl Default for EmbeddingBackend {
    fn default() -> Self {
        Self::Onnx
    }
}

/// Configuration for OpenAI-compatible embedding API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEmbeddingConfig {
    /// API endpoint URL (e.g. "http://localhost:11434/v1/embeddings")
    pub url: String,
    /// API key — literal value or "env:VAR_NAME" to read from environment
    pub api_key: Option<String>,
}

impl ApiEmbeddingConfig {
    /// Resolve the API key, supporting "env:VAR_NAME" syntax
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key.as_ref().and_then(|key| {
            if let Some(var_name) = key.strip_prefix("env:") {
                std::env::var(var_name).ok()
            } else if key.is_empty() {
                None
            } else {
                Some(key.clone())
            }
        })
    }
}

/// Configuration for custom local ONNX models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomModelConfig {
    /// Path to ONNX model file
    pub model_path: String,
    /// Path to tokenizer.json file
    pub tokenizer_path: String,
    /// Maximum sequence length (default: 512)
    pub max_seq_len: Option<usize>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            backend: EmbeddingBackend::default(),
            model: "all-MiniLM-L6-v2".into(),
            batch_size: 32,
            dimensions: None,
            gpu: false,
            api: None,
            custom_model: None,
            context: ContextualEmbeddingConfig::default(),
        }
    }
}

impl EmbeddingConfig {
    /// Check if GPU should be used.
    ///
    /// Resolution order:
    /// 1. `BOBBIN_GPU=0` / `false` → force CPU
    /// 2. `BOBBIN_GPU=1` / `true` → force GPU
    /// 3. Config `gpu = true` → force GPU
    /// 4. Otherwise → auto-detect CUDA availability
    pub fn use_gpu(&self) -> bool {
        if let Ok(v) = std::env::var("BOBBIN_GPU") {
            if v == "0" || v.eq_ignore_ascii_case("false") {
                return false;
            }
            if v == "1" || v.eq_ignore_ascii_case("true") {
                return true;
            }
        }
        if self.gpu {
            return true;
        }
        // Auto-detect: only for ONNX backend
        if self.backend == EmbeddingBackend::Onnx {
            return Self::detect_cuda();
        }
        false
    }

    /// Probe whether the CUDA execution provider is available at runtime.
    fn detect_cuda() -> bool {
        use ort::ep::{ExecutionProvider, CUDA};
        CUDA::default().is_available().unwrap_or(false)
    }
}

/// Configuration for contextual embeddings
///
/// When enabled for a language, chunks are embedded with surrounding context
/// (N lines before/after) for better retrieval quality. The original chunk
/// content is stored for display; the enriched text is stored in full_context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextualEmbeddingConfig {
    /// Number of context lines to include before and after a chunk
    pub context_lines: usize,
    /// Languages where contextual embedding is enabled
    pub enabled_languages: Vec<String>,
}

impl Default for ContextualEmbeddingConfig {
    fn default() -> Self {
        Self {
            context_lines: 5,
            enabled_languages: vec!["markdown".into()],
        }
    }
}

/// Configuration for search behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Default number of results to return
    pub default_limit: usize,
    /// Weight for semantic vs keyword search (0.0 = keyword only, 1.0 = semantic only)
    pub semantic_weight: f32,
    /// Half-life for recency decay in days. After this many days, a result's
    /// recency boost drops to 50%. Set to 0.0 to disable recency boosting.
    pub recency_half_life_days: f32,
    /// How much recency affects final score (0.0 = no effect, 1.0 = full effect).
    /// The final score is: `score * (1.0 - weight + weight * decay)` so at
    /// max weight=1.0, a very old result loses up to 100% of its score.
    /// At default 0.3, the maximum penalty for old results is 30%.
    pub recency_weight: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 10,
            semantic_weight: 0.7,
            recency_half_life_days: 30.0,
            recency_weight: 0.3,
        }
    }
}

/// Configuration for dependency analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DependencyConfig {
    /// Enable dependency extraction and storage
    pub enabled: bool,
    /// Enable import path resolution
    pub resolve_imports: bool,
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            resolve_imports: true,
        }
    }
}

/// Configuration for git analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Enable temporal coupling analysis
    pub coupling_enabled: bool,
    /// How many commits back to analyze
    pub coupling_depth: usize,
    /// Minimum co-changes to establish coupling
    pub coupling_threshold: u32,
    /// Enable semantic commit indexing (embed commit messages for search)
    pub commits_enabled: bool,
    /// How many commits back to index for semantic search (0 = all)
    pub commits_depth: usize,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            coupling_enabled: true,
            coupling_depth: 5000,
            coupling_threshold: 3,
            commits_enabled: true,
            commits_depth: 0,
        }
    }
}


/// Configuration for Claude Code hooks integration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    /// Minimum relevance score to include in injected context
    pub threshold: f32,
    /// Maximum lines of injected context
    pub budget: usize,
    /// Content display mode: "full", "preview", or "none"
    pub content_mode: String,
    /// Skip injection for prompts shorter than this
    pub min_prompt_length: usize,
    /// Minimum raw semantic similarity to inject context at all.
    /// If the top semantic search result scores below this threshold,
    /// the entire injection is skipped (the query isn't relevant enough).
    pub gate_threshold: f32,
    /// Skip injection when search results haven't changed since last injection
    pub dedup_enabled: bool,
    /// Include documentation files in injection output (default: true).
    /// When false, doc files are excluded from output but still used for
    /// provenance bridging to discover relevant source files.
    pub show_docs: bool,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            budget: 300,
            content_mode: "full".into(),
            min_prompt_length: 10,
            gate_threshold: 0.75,
            dedup_enabled: true,
            show_docs: true,
        }
    }
}

/// Configuration for beads (Dolt issue tracker) integration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BeadsConfig {
    /// Enable beads indexing
    pub enabled: bool,
    /// Dolt server hostname
    pub host: String,
    /// Dolt server port
    pub port: u16,
    /// Dolt user
    pub user: String,
    /// Database names to index (e.g., ["beads_aegis", "beads_gastown"])
    pub databases: Vec<String>,
    /// Include comments in indexed content
    pub include_comments: bool,
    /// Include closed beads
    pub include_closed: bool,
    /// Skip beads older than this many days (0 = no limit)
    pub max_age_days: u32,
}

impl Default for BeadsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "dolt.svc".into(),
            port: 3306,
            user: "root".into(),
            databases: vec![],
            include_comments: true,
            include_closed: false,
            max_age_days: 90,
        }
    }
}

/// Configuration for the Human Intent Archive integration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveConfig {
    /// Enable intent archive indexing
    pub enabled: bool,
    /// Path to archive records directory
    pub archive_path: String,
    /// Webhook secret for push notifications (empty = no auth)
    pub webhook_secret: String,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            archive_path: String::new(),
            webhook_secret: String::new(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    /// Save configuration to a TOML file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))
    }

    /// Get the path to the bobbin data directory for a repository
    pub fn data_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".bobbin")
    }

    /// Get the config file path for a repository
    pub fn config_path(repo_root: &Path) -> PathBuf {
        Self::data_dir(repo_root).join("config.toml")
    }

    /// Get the SQLite database path
    pub fn db_path(repo_root: &Path) -> PathBuf {
        Self::data_dir(repo_root).join("index.db")
    }

    /// Get the LanceDB directory path
    pub fn lance_path(repo_root: &Path) -> PathBuf {
        Self::data_dir(repo_root).join("vectors")
    }

    /// Get the global model cache directory
    pub fn model_cache_dir() -> Result<PathBuf> {
        let project_dirs = directories::ProjectDirs::from("dev", "bobbin", "bobbin")
            .context("Failed to determine user directories")?;
        Ok(project_dirs.cache_dir().join("models"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_backward_compatible() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.backend, EmbeddingBackend::Onnx);
        assert_eq!(config.model, "all-MiniLM-L6-v2");
        assert_eq!(config.batch_size, 32);
        assert!(config.dimensions.is_none());
        assert!(config.api.is_none());
        assert!(config.custom_model.is_none());
    }

    #[test]
    fn test_parse_legacy_config() {
        // Old-style config without backend field should still work
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedding.backend, EmbeddingBackend::Onnx);
        assert_eq!(config.embedding.model, "all-MiniLM-L6-v2");
    }

    #[test]
    fn test_parse_openai_api_config() {
        let toml_str = r#"
[embedding]
backend = "openai-api"
model = "nomic-embed-text"
dimensions = 768

[embedding.api]
url = "http://localhost:11434/v1/embeddings"
api_key = "test-key"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedding.backend, EmbeddingBackend::OpenaiApi);
        assert_eq!(config.embedding.model, "nomic-embed-text");
        assert_eq!(config.embedding.dimensions, Some(768));
        let api = config.embedding.api.unwrap();
        assert_eq!(api.url, "http://localhost:11434/v1/embeddings");
        assert_eq!(api.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_parse_custom_onnx_config() {
        let toml_str = r#"
[embedding]
model = "custom"
dimensions = 1024

[embedding.custom_model]
model_path = "/path/to/model.onnx"
tokenizer_path = "/path/to/tokenizer.json"
max_seq_len = 512
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedding.backend, EmbeddingBackend::Onnx);
        assert_eq!(config.embedding.dimensions, Some(1024));
        let custom = config.embedding.custom_model.unwrap();
        assert_eq!(custom.model_path, "/path/to/model.onnx");
        assert_eq!(custom.tokenizer_path, "/path/to/tokenizer.json");
        assert_eq!(custom.max_seq_len, Some(512));
    }

    #[test]
    fn test_api_key_resolve_literal() {
        let api = ApiEmbeddingConfig {
            url: "http://example.com".to_string(),
            api_key: Some("literal-key".to_string()),
        };
        assert_eq!(api.resolve_api_key(), Some("literal-key".to_string()));
    }

    #[test]
    fn test_api_key_resolve_env() {
        std::env::set_var("TEST_BOBBIN_API_KEY", "env-value");
        let api = ApiEmbeddingConfig {
            url: "http://example.com".to_string(),
            api_key: Some("env:TEST_BOBBIN_API_KEY".to_string()),
        };
        assert_eq!(api.resolve_api_key(), Some("env-value".to_string()));
        std::env::remove_var("TEST_BOBBIN_API_KEY");
    }

    #[test]
    fn test_api_key_resolve_empty() {
        let api = ApiEmbeddingConfig {
            url: "http://example.com".to_string(),
            api_key: Some("".to_string()),
        };
        assert!(api.resolve_api_key().is_none());
    }

    #[test]
    fn test_api_key_resolve_none() {
        let api = ApiEmbeddingConfig {
            url: "http://example.com".to_string(),
            api_key: None,
        };
        assert!(api.resolve_api_key().is_none());
    }

    #[test]
    fn test_dependencies_config_default_enabled() {
        let config = Config::default();
        assert!(config.dependencies.enabled);
    }

    #[test]
    fn test_dependencies_config_disabled() {
        let toml_str = r#"
[dependencies]
enabled = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.dependencies.enabled);
    }

    #[test]
    fn test_legacy_config_without_dependencies_section() {
        // Config without [dependencies] should default to enabled
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.dependencies.enabled);
    }

    #[test]
    fn test_git_commits_config_defaults() {
        let config = Config::default();
        assert!(config.git.commits_enabled);
        assert_eq!(config.git.commits_depth, 0);
    }

    #[test]
    fn test_git_commits_config_custom() {
        let toml_str = r#"
[git]
commits_enabled = false
commits_depth = 500
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.git.commits_enabled);
        assert_eq!(config.git.commits_depth, 500);
    }

    #[test]
    fn test_legacy_git_config_without_commits_fields() {
        // Old config without commits fields should default to enabled, depth=0 (all)
        let toml_str = r#"
[git]
coupling_enabled = true
coupling_depth = 500
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.git.commits_enabled);
        assert_eq!(config.git.commits_depth, 0);
    }

    #[test]
    fn test_hooks_config_defaults() {
        let config = Config::default();
        assert!((config.hooks.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.hooks.budget, 300);
        assert_eq!(config.hooks.content_mode, "full");
        assert_eq!(config.hooks.min_prompt_length, 10);
        assert!((config.hooks.gate_threshold - 0.75).abs() < f32::EPSILON);
        assert!(config.hooks.dedup_enabled);
    }

    #[test]
    fn test_hooks_config_custom() {
        let toml_str = r#"
[hooks]
threshold = 0.7
budget = 200
content_mode = "preview"
min_prompt_length = 20
gate_threshold = 0.9
dedup_enabled = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.hooks.threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.hooks.budget, 200);
        assert_eq!(config.hooks.content_mode, "preview");
        assert_eq!(config.hooks.min_prompt_length, 20);
        assert!((config.hooks.gate_threshold - 0.9).abs() < f32::EPSILON);
        assert!(!config.hooks.dedup_enabled);
    }

    #[test]
    fn test_legacy_config_without_hooks_section() {
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.hooks.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.hooks.budget, 300);
        assert!((config.hooks.gate_threshold - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hooks_gate_threshold_backward_compatible() {
        // Config with hooks section but no gate_threshold should default to 0.75
        let toml_str = r#"
[hooks]
threshold = 0.5
budget = 300
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.hooks.gate_threshold - 0.75).abs() < f32::EPSILON);
        assert!(config.hooks.dedup_enabled);
    }

    #[test]
    fn test_backend_serde_roundtrip() {
        let config = EmbeddingConfig {
            backend: EmbeddingBackend::OpenaiApi,
            model: "test".to_string(),
            batch_size: 16,
            dimensions: Some(768),
            api: Some(ApiEmbeddingConfig {
                url: "http://localhost:8080/v1/embeddings".to_string(),
                api_key: None,
            }),
            custom_model: None,
            context: ContextualEmbeddingConfig::default(),
            ..Default::default()
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: EmbeddingConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.backend, EmbeddingBackend::OpenaiApi);
        assert_eq!(deserialized.dimensions, Some(768));
    }
}
