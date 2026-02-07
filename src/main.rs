use anyhow::Result;
use clap::Parser;

mod cli;
mod config;
mod http;
mod index;
mod mcp;
mod search;
mod storage;
mod types;

use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    cli.run().await
}
