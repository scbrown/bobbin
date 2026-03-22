use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main configuration for Bobbin
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub index: IndexConfig,
    pub embedding: EmbeddingConfig,
    pub search: SearchConfig,
    pub git: GitConfig,
    pub dependencies: DependencyConfig,
    pub hooks: HooksConfig,
    pub beads: BeadsConfig,
    pub archive: ArchiveConfig,
    pub access: AccessConfig,
    pub sources: SourcesConfig,
    pub groups: Vec<GroupConfig>,
    pub file_types: Vec<FileTypeRule>,
}

/// Configuration for remote server (thin-client mode)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ServerConfig {
    /// Remote bobbin HTTP server URL (e.g. "http://search.svc")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Bind address for `bobbin serve --http` (default: "0.0.0.0").
    /// Use "127.0.0.1" to restrict to localhost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    /// Filesystem prefix for indexed repos on the server.
    /// Used to normalize absolute paths in search results back to
    /// repo-relative paths (e.g., "/var/lib/bobbin/repos/").
    /// If unset, absolute paths are returned as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path_prefix: Option<String>,
}

/// Global configuration loaded from ~/.config/bobbin/config.toml.
///
/// Uses the same Config struct — supports all config options.
/// This provides defaults for all bobbin invocations on this machine.
/// Per-repo config and CLI flags take precedence.
pub type GlobalConfig = Config;

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
                "**/CONTRIBUTING.md".into(),
                "**/contributing.md".into(),
                "**/searchindex*.js".into(),
                "**/*.min.js".into(),
                "**/*.min.css".into(),
                "**/.scratch/**".into(),
                "**/vendor/**".into(),
                "**/.venv/**".into(),
                "**/book/book/**".into(),
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
        crate::index::embedder::auto_resolve_gpu_dylib();
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
    /// RRF (Reciprocal Rank Fusion) constant `k`. Controls how quickly scores
    /// decay with rank. Lower values make top ranks more dominant; higher values
    /// flatten the distribution. Default: 60.0 (standard RRF).
    pub rrf_k: f32,
    /// Demotion factor for Documentation/Config files in search ranking.
    /// Applied as a multiplier to RRF scores: 1.0 = no demotion, 0.0 = full demotion.
    /// Source/Test files are unaffected. Default: 0.5 (halve doc/config scores).
    pub doc_demotion: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 10,
            semantic_weight: 0.9,
            recency_half_life_days: 30.0,
            recency_weight: 0.3,
            rrf_k: 60.0,
            doc_demotion: 0.3,
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
    /// Injection output format: "standard", "minimal", "verbose", or "xml".
    /// Controls how chunks are presented to agents.
    #[serde(default = "default_format_mode")]
    pub format_mode: String,
    /// Enable session-level progressive reducing: track injected chunks across
    /// turns and only inject new/changed chunks (delta injection).
    #[serde(default = "default_true")]
    pub reducing_enabled: bool,
    /// Keyword-triggered repo scoping rules. When a query matches keywords,
    /// search is scoped to the matched repos instead of all repos.
    #[serde(default)]
    pub keyword_repos: Vec<KeywordRepoRule>,
    /// Score multiplier for files from the agent's current repo (soft affinity).
    /// Default: 2.0. Set to 1.0 to disable.
    #[serde(default = "default_repo_affinity_boost")]
    pub repo_affinity_boost: f32,
    /// Prompt agents to rate injections every N injections (0 = disabled).
    /// Default: 5.
    #[serde(default = "default_feedback_prompt_interval")]
    pub feedback_prompt_interval: u64,
    /// Prompt prefixes that skip injection entirely (case-insensitive).
    /// Use for operational commands that never need context (e.g., "git push",
    /// "bd ready", "gt hook"). Matched as prefix of trimmed prompt.
    #[serde(default)]
    pub skip_prefixes: Vec<String>,
}

/// A rule that maps query keywords to repository names.
/// When any keyword matches (case-insensitive substring), the matched repos
/// are added to the search scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordRepoRule {
    /// Keywords to match against the query (case-insensitive substring match)
    pub keywords: Vec<String>,
    /// Repository names to include when keywords match
    pub repos: Vec<String>,
}

fn default_format_mode() -> String {
    "standard".into()
}

fn default_repo_affinity_boost() -> f32 {
    2.0
}

fn default_feedback_prompt_interval() -> u64 {
    5
}

fn default_true() -> bool {
    true
}

/// Valid format modes for injection output.
pub const VALID_FORMAT_MODES: &[&str] = &["standard", "minimal", "verbose", "xml"];

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            budget: 300,
            content_mode: "full".into(),
            min_prompt_length: 20,
            gate_threshold: 0.45,
            dedup_enabled: true,
            show_docs: true,
            format_mode: "standard".into(),
            reducing_enabled: true,
            keyword_repos: vec![],
            repo_affinity_boost: 2.0,
            feedback_prompt_interval: 5,
            skip_prefixes: vec![],
        }
    }
}

impl HooksConfig {
    /// Resolve keyword-triggered repos from a query string.
    /// Returns a deduplicated list of repo names matched by any keyword rule.
    pub fn resolve_keyword_repos(&self, query: &str) -> Vec<String> {
        if self.keyword_repos.is_empty() {
            return vec![];
        }
        let query_lower = query.to_lowercase();
        let mut repos: Vec<String> = Vec::new();
        for rule in &self.keyword_repos {
            if rule.keywords.iter().any(|kw| query_lower.contains(&kw.to_lowercase())) {
                for repo in &rule.repos {
                    if !repos.contains(repo) {
                        repos.push(repo.clone());
                    }
                }
            }
        }
        repos
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

/// Configuration for archive indexing (human directives, agent memories, etc.)
///
/// Archives are directories of markdown files with YAML frontmatter.
/// Each source is identified by a schema string in the frontmatter and
/// tagged with a configurable label for filtering in search results.
///
/// Example config:
/// ```toml
/// [archive]
/// enabled = true
///
/// [[archive.sources]]
/// name = "hla"
/// path = "/mnt/hla/records"
/// schema = "human-intent"
/// name_field = "channel"
///
/// [[archive.sources]]
/// name = "pensieve"
/// path = "/mnt/pensieve/records"
/// schema = "agent-memory"
/// name_field = "agent"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveConfig {
    /// Enable archive indexing
    pub enabled: bool,
    /// Archive sources to index
    pub sources: Vec<ArchiveSource>,
    /// Webhook secret for push notifications (empty = no auth)
    pub webhook_secret: String,
}

/// A single archive source definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveSource {
    /// Label for this source (used as language tag in chunks, and as path prefix).
    /// Also used as the `source` filter value in search queries.
    pub name: String,
    /// Filesystem path to the archive records directory.
    pub path: String,
    /// String to match in YAML frontmatter to identify records from this source.
    /// E.g., "human-intent" matches `schema: human-intent/v2`.
    pub schema: String,
    /// Frontmatter field to use as the name prefix in chunk names.
    /// E.g., "channel" → chunk name becomes "{channel_value}/{record_id}".
    /// If empty, chunk name is just the record ID.
    #[serde(default)]
    pub name_field: String,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sources: vec![],
            webhook_secret: String::new(),
        }
    }
}

/// Configuration for role-based repository access filtering (§69)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessConfig {
    /// When true, repos not listed in any rule are visible to all roles.
    /// When false, repos must be explicitly granted.
    pub default_allow: bool,
    /// Role definitions with allow/deny lists
    pub roles: Vec<RoleConfig>,
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self {
            default_allow: true,
            roles: vec![],
        }
    }
}

/// A single role definition with repo allow/deny lists
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    /// Role name or pattern (e.g. "human", "aegis/*", "bobbin/polecats/*")
    pub name: String,
    /// Repos this role can see (glob patterns, e.g. ["*"], ["aegis", "bobbin"])
    #[serde(default)]
    pub allow: Vec<String>,
    /// Repos this role cannot see (glob patterns, deny takes precedence over allow)
    #[serde(default)]
    pub deny: Vec<String>,
    /// File path patterns to deny within allowed repos (glob patterns)
    #[serde(default)]
    pub deny_paths: Vec<String>,
}

/// Maps indexed repo short names to their web browse URLs.
///
/// Example config:
/// ```toml
/// [sources]
/// # Template for auto-detected git remotes. {remote_base} is the web URL
/// # of the repo (e.g. "https://github.com/owner/repo").
/// # Forgejo/Gitea:
/// remote_template = "{remote_base}/src/branch/main/{path}#L{line}"
/// # GitHub:
/// # remote_template = "{remote_base}/blob/main/{path}#L{line}"
///
/// # Fallback template for repos with no git remote (uses {repo}, {path}, {line})
/// default_url = ""
///
/// # Per-repo overrides (full URL templates, highest priority)
/// [sources.repos]
/// beads = "https://github.com/scbrown/beads/blob/main/{path}#L{line}"
/// ```
///
/// URL templates support `{repo}`, `{path}`, `{line}`, and `{remote_base}` placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    /// Template applied to auto-detected git remotes.
    /// Placeholders: {remote_base} (web base URL), {path}, {line}
    /// Example: "{remote_base}/src/branch/main/{path}#L{line}"
    #[serde(default)]
    pub remote_template: String,
    /// Default URL template for repos not explicitly listed and without a git remote.
    /// Placeholders: {repo}, {path}, {line}
    pub default_url: String,
    /// Per-repo URL overrides. Key = repo short name, value = URL template.
    /// These take priority over auto-detection.
    pub repos: std::collections::HashMap<String, String>,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            remote_template: String::new(),
            default_url: String::new(),
            repos: std::collections::HashMap::new(),
        }
    }
}

/// Named repo group for scoped search.
///
/// Groups define named sets of repositories that can be used to narrow
/// search scope via `--group` (CLI) or `?group=` (HTTP). Groups compose
/// with role-based access filtering — a group can only include repos the
/// caller is allowed to see.
///
/// Example config:
/// ```toml
/// [[groups]]
/// name = "infra"
/// repos = ["goldblum", "homelab-mcp", "aegis"]
///
/// [[groups]]
/// name = "apps"
/// repos = ["reckoning", "tapestry", "shanty"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupConfig {
    /// Group name (used in --group flag and ?group= param)
    pub name: String,
    /// Repository names in this group
    pub repos: Vec<String>,
}

/// Configurable file type classification rule.
///
/// Maps glob patterns to a file category name. Rules are evaluated in order;
/// first match wins. Files not matched by any rule fall through to the
/// built-in `classify_file()` heuristics.
///
/// Example config:
/// ```toml
/// [[file_types]]
/// name = "config"
/// patterns = ["*.toml", "*.yaml", "*.yml", ".env*", "Dockerfile*"]
///
/// [[file_types]]
/// name = "test"
/// patterns = ["tests/**", "*_test.go", "test_*.py"]
///
/// [[file_types]]
/// name = "documentation"
/// patterns = ["docs/**/*.md", "README*", "CHANGELOG*"]
///
/// [[file_types]]
/// name = "generated"
/// patterns = ["*.pb.go", "*.generated.ts", "migrations/*.sql"]
/// ```
///
/// Built-in category names: "source", "test", "documentation", "config".
/// Custom names (e.g., "generated", "vendor", "schema") are also supported
/// and will be stored/displayed as-is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeRule {
    /// Category name (e.g., "source", "test", "documentation", "config", or custom)
    pub name: String,
    /// Glob patterns that match this category (matched against repo-relative paths)
    pub patterns: Vec<String>,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    /// Look up a group by name. Returns the repo list, or None if not found.
    pub fn resolve_group(&self, name: &str) -> Option<&[String]> {
        self.groups
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.repos.as_slice())
    }

    /// Build a SQL filter clause for a group's repos: `repo IN ('a', 'b', 'c')`.
    /// Returns None if group not found or empty.
    pub fn group_filter(&self, name: &str) -> Option<String> {
        self.resolve_group(name).and_then(|repos| {
            if repos.is_empty() {
                return None;
            }
            let escaped: Vec<String> = repos
                .iter()
                .map(|r| format!("'{}'", r.replace('\'', "''")))
                .collect();
            Some(format!("repo IN ({})", escaped.join(", ")))
        })
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

    /// Get the feedback SQLite database path
    pub fn feedback_db_path(repo_root: &Path) -> PathBuf {
        Self::data_dir(repo_root).join("feedback.db")
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

    /// Get the global config directory (~/.config/bobbin/)
    pub fn global_config_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("dev", "bobbin", "bobbin")
            .map(|dirs| dirs.config_dir().to_path_buf())
    }

    /// Get the global config file path (~/.config/bobbin/config.toml)
    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_config_dir().map(|dir| dir.join("config.toml"))
    }
}

// --- Global config helpers (on Config, used via GlobalConfig alias) ---

impl Config {
    /// Load global config from ~/.config/bobbin/config.toml.
    /// Returns default (empty) config if file doesn't exist.
    pub fn load_global() -> Config {
        Self::global_config_path()
            .and_then(|path| {
                if path.exists() {
                    std::fs::read_to_string(&path).ok()
                } else {
                    None
                }
            })
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Save this config as the global config at ~/.config/bobbin/config.toml.
    pub fn save_global(&self) -> Result<()> {
        let path = Self::global_config_path()
            .context("Failed to determine global config path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create global config directory: {}", parent.display())
            })?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize global config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write global config: {}", path.display()))
    }

    /// Load config with full hierarchy: global defaults < per-repo config.
    ///
    /// Resolution order (last wins):
    /// 1. Compiled defaults (`Config::default()`)
    /// 2. Global config (`~/.config/bobbin/config.toml`)
    /// 3. Per-repo config (`.bobbin/config.toml`)
    ///
    /// Per-repo config is loaded as a TOML overlay — only fields explicitly
    /// set in the file override the global values. Fields absent from the
    /// per-repo file retain their global (or default) values.
    pub fn load_merged(repo_root: &Path) -> Result<Self> {
        // Start with global config (includes compiled defaults for missing fields)
        let global = Self::load_global();

        // Overlay per-repo config
        let repo_config_path = Self::config_path(repo_root);
        if repo_config_path.exists() {
            let repo_toml = std::fs::read_to_string(&repo_config_path)
                .with_context(|| format!("Failed to read {}", repo_config_path.display()))?;
            // Parse the raw TOML to see which keys are explicitly set
            let repo_table: toml::Value = toml::from_str(&repo_toml)
                .with_context(|| format!("Failed to parse {}", repo_config_path.display()))?;
            // Serialize global config to TOML value, then deep-merge repo on top
            let global_table = toml::Value::try_from(&global)
                .context("Failed to serialize global config for merge")?;
            let merged = deep_merge_toml(global_table, repo_table);
            merged.try_into()
                .context("Failed to deserialize merged config")
        } else {
            Ok(global)
        }
    }
}

/// Deep-merge two TOML values. `overlay` keys override `base` keys.
/// Tables are merged recursively; all other types are replaced wholesale.
fn deep_merge_toml(base: toml::Value, overlay: toml::Value) -> toml::Value {
    match (base, overlay) {
        (toml::Value::Table(mut base_map), toml::Value::Table(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let merged = if let Some(base_val) = base_map.remove(&key) {
                    deep_merge_toml(base_val, overlay_val)
                } else {
                    overlay_val
                };
                base_map.insert(key, merged);
            }
            toml::Value::Table(base_map)
        }
        // Non-table overlay replaces base entirely
        (_base, overlay) => overlay,
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
    fn test_search_config_defaults() {
        let config = Config::default();
        assert_eq!(config.search.default_limit, 10);
        assert!((config.search.semantic_weight - 0.9).abs() < f32::EPSILON);
        assert!((config.search.recency_half_life_days - 30.0).abs() < f32::EPSILON);
        assert!((config.search.recency_weight - 0.3).abs() < f32::EPSILON);
        assert!((config.search.rrf_k - 60.0).abs() < f32::EPSILON);
        assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_search_config_custom() {
        let toml_str = r#"
[search]
semantic_weight = 0.5
rrf_k = 40.0
doc_demotion = 0.3
recency_weight = 0.1
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.search.semantic_weight - 0.5).abs() < f32::EPSILON);
        assert!((config.search.rrf_k - 40.0).abs() < f32::EPSILON);
        assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
        assert!((config.search.recency_weight - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn test_legacy_config_without_search_tuning_fields() {
        // Old config without rrf_k/doc_demotion should use defaults
        let toml_str = r#"
[search]
semantic_weight = 0.8
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.search.semantic_weight - 0.8).abs() < f32::EPSILON);
        assert!((config.search.rrf_k - 60.0).abs() < f32::EPSILON);
        assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hooks_config_defaults() {
        let config = Config::default();
        assert!((config.hooks.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.hooks.budget, 300);
        assert_eq!(config.hooks.content_mode, "full");
        assert_eq!(config.hooks.min_prompt_length, 20);
        assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
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
        assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hooks_gate_threshold_backward_compatible() {
        // Config with hooks section but no gate_threshold should default to 0.65
        let toml_str = r#"
[hooks]
threshold = 0.5
budget = 300
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
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

    #[test]
    fn test_server_config_default_is_none() {
        let config = Config::default();
        assert!(config.server.url.is_none());
    }

    #[test]
    fn test_server_config_parse() {
        let toml_str = r#"
[server]
url = "http://search.svc"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.url.as_deref(), Some("http://search.svc"));
    }

    #[test]
    fn test_legacy_config_without_server_section() {
        // Old config without [server] should still parse fine
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.server.url.is_none());
    }

    #[test]
    fn test_global_config_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");

        let mut config = Config::default();
        config.server.url = Some("http://search.svc".to_string());

        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, &content).unwrap();

        let loaded: Config =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(loaded.server.url.as_deref(), Some("http://search.svc"));
    }

    #[test]
    fn test_groups_config_parse() {
        let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "homelab-mcp", "aegis"]

[[groups]]
name = "apps"
repos = ["reckoning", "tapestry"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups[0].name, "infra");
        assert_eq!(config.groups[0].repos, vec!["goldblum", "homelab-mcp", "aegis"]);
        assert_eq!(config.groups[1].name, "apps");
        assert_eq!(config.groups[1].repos, vec!["reckoning", "tapestry"]);
    }

    #[test]
    fn test_groups_default_empty() {
        let config = Config::default();
        assert!(config.groups.is_empty());
    }

    #[test]
    fn test_legacy_config_without_groups() {
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.groups.is_empty());
    }

    #[test]
    fn test_resolve_group() {
        let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "aegis"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.resolve_group("infra"), Some(&["goldblum".to_string(), "aegis".to_string()][..]));
        assert_eq!(config.resolve_group("nonexistent"), None);
    }

    #[test]
    fn test_group_filter_sql() {
        let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "homelab-mcp"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.group_filter("infra"),
            Some("repo IN ('goldblum', 'homelab-mcp')".to_string())
        );
        assert_eq!(config.group_filter("nonexistent"), None);
    }

    #[test]
    fn test_group_filter_empty_repos() {
        let toml_str = r#"
[[groups]]
name = "empty"
repos = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.group_filter("empty"), None);
    }

    #[test]
    fn test_keyword_repos_config_parse() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible", "playbook"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads", "bd "]
repos = ["beads"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hooks.keyword_repos.len(), 2);
        assert_eq!(config.hooks.keyword_repos[0].keywords, vec!["ansible", "playbook"]);
        assert_eq!(config.hooks.keyword_repos[0].repos, vec!["goldblum"]);
    }

    #[test]
    fn test_keyword_repos_resolve_match() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible", "playbook"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads", "bd "]
repos = ["beads"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let repos = config.hooks.resolve_keyword_repos("deploy the ansible playbook");
        assert_eq!(repos, vec!["goldblum"]);
    }

    #[test]
    fn test_keyword_repos_resolve_multiple() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads"]
repos = ["beads"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let repos = config.hooks.resolve_keyword_repos("check ansible and beads status");
        assert_eq!(repos, vec!["goldblum", "beads"]);
    }

    #[test]
    fn test_keyword_repos_resolve_case_insensitive() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["Ansible"]
repos = ["goldblum"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let repos = config.hooks.resolve_keyword_repos("ANSIBLE playbook");
        assert_eq!(repos, vec!["goldblum"]);
    }

    #[test]
    fn test_keyword_repos_resolve_no_match() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let repos = config.hooks.resolve_keyword_repos("fix the bobbin build");
        assert!(repos.is_empty());
    }

    #[test]
    fn test_keyword_repos_dedup() {
        let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["iac"]
repos = ["goldblum"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let repos = config.hooks.resolve_keyword_repos("ansible iac deployment");
        assert_eq!(repos, vec!["goldblum"]);
    }

    #[test]
    fn test_keyword_repos_default_empty() {
        let config = Config::default();
        assert!(config.hooks.keyword_repos.is_empty());
        assert!(config.hooks.resolve_keyword_repos("anything").is_empty());
    }

    #[test]
    fn test_legacy_config_without_keyword_repos() {
        let toml_str = r#"
[hooks]
threshold = 0.5
budget = 300
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.hooks.keyword_repos.is_empty());
    }

    #[test]
    fn test_file_types_config_parse() {
        let toml_str = r#"
[[file_types]]
name = "generated"
patterns = ["*.pb.go", "*.generated.ts"]

[[file_types]]
name = "config"
patterns = ["deploy/*.yaml", "*.toml"]

[[file_types]]
name = "vendor"
patterns = ["vendor/**"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.file_types.len(), 3);
        assert_eq!(config.file_types[0].name, "generated");
        assert_eq!(config.file_types[0].patterns, vec!["*.pb.go", "*.generated.ts"]);
        assert_eq!(config.file_types[1].name, "config");
        assert_eq!(config.file_types[2].name, "vendor");
    }

    #[test]
    fn test_file_types_default_empty() {
        let config = Config::default();
        assert!(config.file_types.is_empty());
    }

    #[test]
    fn test_legacy_config_without_file_types() {
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.file_types.is_empty());
    }

    #[test]
    fn test_deep_merge_toml_overlay_scalar() {
        let base: toml::Value = toml::from_str(r#"
[search]
semantic_weight = 0.9
doc_demotion = 0.3
"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"
[search]
semantic_weight = 0.7
"#).unwrap();
        let merged = deep_merge_toml(base, overlay);
        let config: Config = merged.try_into().unwrap();
        assert!((config.search.semantic_weight - 0.7).abs() < f32::EPSILON);
        // doc_demotion should retain the base value
        assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_deep_merge_toml_overlay_adds_section() {
        let base: toml::Value = toml::from_str(r#"
[search]
semantic_weight = 0.9
"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"
[server]
url = "http://search.svc"
"#).unwrap();
        let merged = deep_merge_toml(base, overlay);
        let config: Config = merged.try_into().unwrap();
        assert_eq!(config.server.url.as_deref(), Some("http://search.svc"));
        // search.semantic_weight from base is preserved
        assert!((config.search.semantic_weight - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_deep_merge_toml_array_replaces() {
        let base: toml::Value = toml::from_str(r#"
[index]
include = ["**/*.rs"]
"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"
[index]
include = ["**/*.py", "**/*.go"]
"#).unwrap();
        let merged = deep_merge_toml(base, overlay);
        let config: Config = merged.try_into().unwrap();
        // Arrays are replaced wholesale, not merged
        assert_eq!(config.index.include, vec!["**/*.py", "**/*.go"]);
    }
}
