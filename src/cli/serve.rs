use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use super::OutputConfig;

#[derive(Args)]
pub struct ServeArgs {
    /// Directory to serve (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Run HTTP server instead of MCP stdio server
    #[arg(long)]
    http: bool,

    /// HTTP server port (default: 3030)
    #[arg(long, default_value = "3030")]
    port: u16,

    /// Run MCP server alongside HTTP server
    #[arg(long)]
    mcp: bool,

    /// Run MCP server over HTTP transport (Streamable HTTP, network-accessible)
    #[arg(long)]
    mcp_http: bool,

    /// Port for MCP HTTP transport (default: 3031)
    #[arg(long, default_value = "3031")]
    mcp_port: u16,
}

pub async fn run(args: ServeArgs, _output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?;

    match (args.http, args.mcp, args.mcp_http) {
        // HTTP API + MCP stdio: run both concurrently
        (true, true, false) => {
            let http_root = repo_root.clone();
            let http_port = args.port;
            let mcp_root = repo_root;

            tokio::select! {
                result = crate::http::run_server(http_root, http_port) => {
                    result?;
                }
                result = crate::mcp::run_server(mcp_root) => {
                    result?;
                }
            }

            Ok(())
        }

        // HTTP API + MCP over HTTP: run both concurrently
        (true, _, true) => {
            let http_root = repo_root.clone();
            let http_port = args.port;
            let mcp_root = repo_root;
            let mcp_port = args.mcp_port;

            tokio::select! {
                result = crate::http::run_server(http_root, http_port) => {
                    result?;
                }
                result = crate::mcp::run_http_server(mcp_root, mcp_port) => {
                    result?;
                }
            }

            Ok(())
        }

        // MCP over HTTP only
        (false, _, true) => crate::mcp::run_http_server(repo_root, args.mcp_port).await,

        // HTTP API only
        (true, false, false) => crate::http::run_server(repo_root, args.port).await,

        // MCP stdio only (default)
        _ => crate::mcp::run_server(repo_root).await,
    }
}
