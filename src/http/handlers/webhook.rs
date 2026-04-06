//! Webhook handler and source URL resolution.
#![allow(private_interfaces)]

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::storage::VectorStore;

use super::AppState;

// ---------------------------------------------------------------------------
// Source URL resolution (called at startup from http/mod.rs)
// ---------------------------------------------------------------------------

/// Known forge types with their URL patterns for file browsing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ForgeType {
    GitHub,
    GitLab,
    Forgejo, // also covers Gitea
    Bitbucket,
}

impl ForgeType {
    /// Returns the URL template suffix for browsing a file at a line.
    /// Template uses `{path}` and `{line}` placeholders.
    fn file_template(&self) -> &'static str {
        match self {
            ForgeType::GitHub => "/blob/main/{path}#L{line}",
            ForgeType::GitLab => "/-/blob/main/{path}#L{line}",
            ForgeType::Forgejo => "/src/branch/main/{path}#L{line}",
            ForgeType::Bitbucket => "/src/main/{path}#{path}-{line}",
        }
    }
}

/// Detect the forge type from a hostname.
///
/// Uses well-known SaaS hosts plus heuristics for self-hosted instances.
fn detect_forge(host: &str) -> ForgeType {
    let h = host.to_lowercase();
    if h == "github.com" || h.ends_with(".github.com") {
        return ForgeType::GitHub;
    }
    if h == "gitlab.com" || h.ends_with(".gitlab.com") {
        return ForgeType::GitLab;
    }
    if h == "bitbucket.org" || h.ends_with(".bitbucket.org") {
        return ForgeType::Bitbucket;
    }
    // Self-hosted: check if host contains forge name hints
    if h.contains("gitlab") {
        return ForgeType::GitLab;
    }
    if h.contains("bitbucket") {
        return ForgeType::Bitbucket;
    }
    // Default to Forgejo/Gitea for self-hosted (most common for homelab)
    ForgeType::Forgejo
}

/// Extract the hostname from a web base URL.
fn host_from_url(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    // host might include port
    Some(rest.split('/').next()?.split(':').next()?)
}

/// Extract a web base URL from a git remote URL.
///
/// Converts SSH (`git@host:owner/repo.git`) and HTTPS (`https://host/owner/repo.git`)
/// remotes into a plain web base URL (`https://host/owner/repo`).
/// Returns None if the remote URL can't be parsed.
fn web_base_from_remote(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches(".git");

    // SSH: git@host:owner/repo.git
    if let Some(rest) = trimmed.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = path.trim_start_matches('/');
        if path.split('/').count() < 2 {
            return None;
        }
        return Some(format!("https://{host}/{path}"));
    }

    // HTTPS: http(s)://host[:port]/owner/repo.git
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        return Some(trimmed.to_string());
    }

    None
}

/// Build a full file-browse URL template from a base URL.
///
/// If `remote_template` is configured, uses it. Otherwise auto-detects the
/// forge type from the host and applies the correct URL pattern.
/// `forge_overrides` lets users override detection for specific hosts.
fn build_source_url(
    base: &str,
    remote_template: &str,
    repo_name: &str,
    forge_overrides: &std::collections::HashMap<String, String>,
) -> String {
    if !remote_template.is_empty() {
        return remote_template
            .replace("{remote_base}", base)
            .replace("{repo}", repo_name);
    }

    // Auto-detect forge type from the base URL host
    let forge = host_from_url(base)
        .map(|host| {
            // Check user overrides first
            if let Some(override_type) = forge_overrides.get(host) {
                match override_type.to_lowercase().as_str() {
                    "github" => ForgeType::GitHub,
                    "gitlab" => ForgeType::GitLab,
                    "forgejo" | "gitea" => ForgeType::Forgejo,
                    "bitbucket" => ForgeType::Bitbucket,
                    _ => detect_forge(host),
                }
            } else {
                detect_forge(host)
            }
        })
        .unwrap_or(ForgeType::Forgejo);

    format!("{}{}", base.trim_end_matches('/'), forge.file_template())
}

/// Auto-detect source URLs from git remotes for all indexed repos,
/// merged with manual overrides from config. Called once at startup.
///
/// For each repo without an explicit `[sources.repos]` entry, reads its git
/// remote origin, extracts a web base URL, and auto-detects the forge type
/// (GitHub/GitLab/Forgejo/Bitbucket) to build correct deep links.
pub(crate) fn resolve_sources(
    repo_root: &std::path::Path,
    config_sources: &crate::config::SourcesConfig,
) -> crate::config::SourcesConfig {
    let mut sources = config_sources.clone();
    let repos_dir = repo_root.join("repos");
    if let Ok(entries) = std::fs::read_dir(&repos_dir) {
        for entry in entries.flatten() {
            let repo_name = entry.file_name().to_string_lossy().to_string();
            if sources.repos.contains_key(&repo_name) {
                continue;
            }
            let repo_path = entry.path();
            if let Ok(output) = std::process::Command::new("git")
                .args(["-C", &repo_path.to_string_lossy(), "remote", "get-url", "origin"])
                .output()
            {
                if output.status.success() {
                    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if let Some(base) = web_base_from_remote(&remote) {
                        let url = build_source_url(
                            &base,
                            &sources.remote_template,
                            &repo_name,
                            &sources.forge_overrides,
                        );
                        sources.repos.insert(repo_name, url);
                    }
                } else {
                    sources.repos.insert(repo_name, String::new());
                }
            } else {
                sources.repos.insert(repo_name, String::new());
            }
        }
    }
    sources
}

// ---------------------------------------------------------------------------
// /webhook/push
// ---------------------------------------------------------------------------

/// Forgejo push webhook payload (subset of fields we care about)
#[derive(Deserialize)]
pub(super) struct ForgejoPushPayload {
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    repository: Option<RepoInfo>,
}

#[derive(Deserialize)]
pub(super) struct RepoInfo {
    full_name: Option<String>,
}

#[derive(Serialize)]
pub(super) struct WebhookResponse {
    status: String,
    message: String,
}

pub(super) async fn webhook_push(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ForgejoPushPayload>,
) -> impl axum::response::IntoResponse {
    let repo_name = payload
        .repository
        .as_ref()
        .and_then(|r| r.full_name.as_deref())
        .unwrap_or("unknown");

    let git_ref = payload.git_ref.as_deref().unwrap_or("unknown");

    tracing::info!(
        repo = repo_name,
        git_ref = git_ref,
        "Received push webhook"
    );

    // Try to map repo full_name to our indexed repo name
    // e.g., "stiwi/bobbin" -> "bobbin"
    let short_repo = repo_name.rsplit('/').next().unwrap_or(repo_name).to_string();

    // Verify this repo is indexed
    let repos_dir = state.repo_root.join("repos");
    let repo_dir = repos_dir.join(&short_repo);
    if !repo_dir.exists() {
        tracing::warn!(repo = short_repo, "Webhook for non-indexed repo, skipping");
        return Json(WebhookResponse {
            status: "skipped".to_string(),
            message: format!("Repo '{}' not indexed", short_repo),
        });
    }

    // Pull latest changes
    let pull_result = std::process::Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "pull", "--ff-only"])
        .output();

    match pull_result {
        Ok(output) if output.status.success() => {
            tracing::info!(repo = short_repo, "Git pull succeeded");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(repo = short_repo, error = %stderr, "Git pull failed");
        }
        Err(e) => {
            tracing::warn!(repo = short_repo, error = %e, "Failed to run git pull");
        }
    }

    // Queue background re-index
    let repo_root = state.repo_root.clone();
    let config = state.config.clone();
    let source_dir = repo_dir.clone();
    let repo_tag = short_repo.clone();
    tokio::spawn(async move {
        if let Err(e) = run_incremental_index(repo_root, config, &source_dir, &repo_tag).await {
            tracing::error!(repo = short_repo, error = %e, "Background re-index failed");
        } else {
            tracing::info!("Background re-index completed for {}", short_repo);
        }
    });

    Json(WebhookResponse {
        status: "accepted".to_string(),
        message: format!("Re-index queued for push to {}", git_ref),
    })
}

/// Run incremental indexing for a specific repo (used by webhook handler).
///
/// `source_dir` is the directory to walk (e.g., `/var/lib/bobbin/repos/gastown`).
/// `repo_name` is the tag stored in the vector DB (e.g., `"gastown"`).
async fn run_incremental_index(
    repo_root: PathBuf,
    config: crate::config::Config,
    source_dir: &std::path::Path,
    repo_name: &str,
) -> anyhow::Result<()> {
    use crate::index::{Embedder, Parser};
    use ignore::WalkBuilder;
    use sha2::{Digest, Sha256};
    use std::path::Path;
    let lance_path = Config::lance_path(&repo_root);
    let mut vector_store = VectorStore::open(&lance_path).await?;



    let model_dir = Config::model_cache_dir()?;
    let embedder = Embedder::load(&model_dir, &config.embedding.model)?;
    let mut parser = Parser::new()?;

    // Collect files
    let mut walker = WalkBuilder::new(source_dir);
    walker.hidden(true).git_ignore(config.index.use_gitignore);

    let include_globs: Vec<glob::Pattern> = config
        .index
        .include
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();
    let exclude_globs: Vec<glob::Pattern> = config
        .index
        .exclude
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();

    let mut files_to_index = Vec::new();
    for entry in walker.build().flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel_path = path
            .strip_prefix(source_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let matches_include = include_globs.is_empty()
            || include_globs.iter().any(|g| g.matches(&rel_path));
        let matches_exclude = exclude_globs.iter().any(|g| g.matches(&rel_path));

        if matches_include && !matches_exclude {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
            let needs = vector_store
                .needs_reindex(&rel_path, &hash)
                .await
                .unwrap_or(true);
            if needs {
                files_to_index.push((rel_path, content, hash));
            }
        }
    }

    tracing::info!(
        "Webhook index: {} files need re-indexing",
        files_to_index.len()
    );

    for (rel_path, content, hash) in &files_to_index {
        let chunks = match parser.parse_file(Path::new(rel_path), content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", rel_path, e);
                continue;
            }
        };
        if chunks.is_empty() {
            continue;
        }

        vector_store.delete_by_file(&[rel_path.clone()]).await?;

        let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings = embedder.embed_batch(&texts).await?;
        let contexts: Vec<Option<String>> = vec![None; chunks.len()];
        let now = chrono::Utc::now().to_rfc3339();

        vector_store
            .insert(&chunks, &embeddings, &contexts, repo_name, hash, &now)
            .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_base_from_remote_ssh() {
        assert_eq!(
            web_base_from_remote("git@github.com:owner/repo.git"),
            Some("https://github.com/owner/repo".to_string())
        );
    }

    #[test]
    fn test_web_base_from_remote_https() {
        assert_eq!(
            web_base_from_remote("https://github.com/owner/repo.git"),
            Some("https://github.com/owner/repo".to_string())
        );
    }

    #[test]
    fn test_web_base_from_remote_http() {
        assert_eq!(
            web_base_from_remote("http://git.svc:3000/stiwi/bobbin"),
            Some("http://git.svc:3000/stiwi/bobbin".to_string())
        );
    }

    #[test]
    fn test_detect_forge_github() {
        assert_eq!(detect_forge("github.com"), ForgeType::GitHub);
    }

    #[test]
    fn test_detect_forge_gitlab() {
        assert_eq!(detect_forge("gitlab.com"), ForgeType::GitLab);
        assert_eq!(detect_forge("gitlab.internal.corp"), ForgeType::GitLab);
    }

    #[test]
    fn test_detect_forge_bitbucket() {
        assert_eq!(detect_forge("bitbucket.org"), ForgeType::Bitbucket);
    }

    #[test]
    fn test_detect_forge_selfhosted_default() {
        // Unknown self-hosted → Forgejo
        assert_eq!(detect_forge("git.svc"), ForgeType::Forgejo);
        assert_eq!(detect_forge("code.internal.com"), ForgeType::Forgejo);
    }

    #[test]
    fn test_build_source_url_auto_github() {
        let overrides = std::collections::HashMap::new();
        let url = build_source_url(
            "https://github.com/owner/repo",
            "",
            "repo",
            &overrides,
        );
        assert_eq!(url, "https://github.com/owner/repo/blob/main/{path}#L{line}");
    }

    #[test]
    fn test_build_source_url_auto_forgejo() {
        let overrides = std::collections::HashMap::new();
        let url = build_source_url(
            "http://git.svc:3000/stiwi/bobbin",
            "",
            "bobbin",
            &overrides,
        );
        assert_eq!(url, "http://git.svc:3000/stiwi/bobbin/src/branch/main/{path}#L{line}");
    }

    #[test]
    fn test_build_source_url_with_template_override() {
        let overrides = std::collections::HashMap::new();
        let url = build_source_url(
            "https://github.com/owner/repo",
            "{remote_base}/tree/develop/{path}#L{line}",
            "repo",
            &overrides,
        );
        assert_eq!(url, "https://github.com/owner/repo/tree/develop/{path}#L{line}");
    }

    #[test]
    fn test_build_source_url_with_forge_override() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("git.svc".to_string(), "gitlab".to_string());
        let url = build_source_url(
            "http://git.svc:3000/stiwi/bobbin",
            "",
            "bobbin",
            &overrides,
        );
        // git.svc overridden to gitlab
        assert_eq!(url, "http://git.svc:3000/stiwi/bobbin/-/blob/main/{path}#L{line}");
    }

    #[test]
    fn test_host_from_url() {
        assert_eq!(host_from_url("https://github.com/owner/repo"), Some("github.com"));
        assert_eq!(host_from_url("http://git.svc:3000/stiwi/bobbin"), Some("git.svc"));
        assert_eq!(host_from_url("not-a-url"), None);
    }
}
