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
    /// Model name or path to ONNX file
    pub model: String,
    /// Batch size for embedding generation
    pub batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "all-MiniLM-L6-v2".into(),
            batch_size: 32,
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
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 10,
            semantic_weight: 0.7,
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
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            coupling_enabled: true,
            coupling_depth: 1000,
            coupling_threshold: 3,
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
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
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
}
