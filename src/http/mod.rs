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

use crate::config::Config;

/// Shared application state for HTTP handlers
pub struct AppState {
    pub repo_root: PathBuf,
    pub config: Config,
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

    let state = Arc::new(AppState { repo_root, config });

    let app = handlers::router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Bobbin HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}
