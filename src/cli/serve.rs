use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use super::OutputConfig;

#[derive(Args)]
pub struct ServeArgs {
    /// Directory to serve (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

pub async fn run(args: ServeArgs, _output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("Invalid path: {}", e))?;

    // Run the MCP server
    crate::mcp::run_server(repo_root).await
}
