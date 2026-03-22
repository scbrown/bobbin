//! HTTP server and client for Bobbin.
//!
//! Provides a REST API for code search and analysis, allowing centralized
//! deployment with thin CLI clients and webhook-driven indexing.

pub mod client;
mod handlers;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::OnceCell;

use crate::config::{Config, SourcesConfig};
use crate::index::embedder::Embedder;
use crate::tags::TagsConfig;

/// Shared application state for HTTP handlers
pub struct AppState {
    pub repo_root: PathBuf,
    pub config: Config,
    /// Source URLs resolved once at startup (auto-detected + manual overrides).
    pub resolved_sources: SourcesConfig,
    /// Inner router for /cmd dispatch (set once after router construction).
    pub inner_router: std::sync::OnceLock<axum::Router>,
    /// Tags configuration for effect-based scoring/exclusion.
    pub tags_config: TagsConfig,
    /// Cached Embedder — ONNX model loaded once, reused across requests.
    /// Previously, every request loaded the model from disk (~90MB).
    pub embedder: OnceCell<Embedder>,
}

impl AppState {
    /// Get or initialize the cached Embedder.
    pub async fn get_embedder(&self) -> Result<&Embedder> {
        self.embedder
            .get_or_try_init(|| async {
                let model_dir = Config::model_cache_dir()?;
                Embedder::from_config(&self.config.embedding, &model_dir)
            })
            .await
    }
}

/// Run the HTTP server on the given port
pub async fn run_server(repo_root: PathBuf, port: u16) -> Result<()> {
    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        anyhow::bail!(
            "Bobbin not initialized in {}. Run `bobbin init` first.",
            repo_root.display()
        );
    }

    let config = Config::load(&config_path).context("Failed to load config")?;
    let bind_addr: std::net::IpAddr = config
        .server
        .bind_address
        .as_deref()
        .unwrap_or("0.0.0.0")
        .parse()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    let resolved_sources = handlers::resolve_sources(&repo_root, &config.sources);
    let tags_config = TagsConfig::load_or_default(&TagsConfig::tags_path(&repo_root));

    let state = Arc::new(AppState {
        repo_root,
        config,
        resolved_sources,
        inner_router: std::sync::OnceLock::new(),
        tags_config,
        embedder: OnceCell::new(),
    });

    let app = handlers::router(state);

    let addr = SocketAddr::from((bind_addr, port));
    tracing::info!("Bobbin HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}
