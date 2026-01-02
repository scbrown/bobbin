mod init;
mod index;
mod search;
mod grep;
mod related;
mod status;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bobbin")]
#[command(about = "Local-first code context engine with Temporal RAG")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,

    /// Suppress non-essential output
    #[arg(long, global = true)]
    quiet: bool,

    /// Show detailed progress
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize bobbin in the current repository
    Init(init::InitArgs),

    /// Build or update the search index
    Index(index::IndexArgs),

    /// Semantic search for code
    Search(search::SearchArgs),

    /// Keyword/regex search
    Grep(grep::GrepArgs),

    /// Find files related to a given file
    Related(related::RelatedArgs),

    /// Show index status and statistics
    Status(status::StatusArgs),
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let output = OutputConfig {
            json: self.json,
            quiet: self.quiet,
            verbose: self.verbose,
        };

        match self.command {
            Commands::Init(args) => init::run(args, output).await,
            Commands::Index(args) => index::run(args, output).await,
            Commands::Search(args) => search::run(args, output).await,
            Commands::Grep(args) => grep::run(args, output).await,
            Commands::Related(args) => related::run(args, output).await,
            Commands::Status(args) => status::run(args, output).await,
        }
    }
}

/// Output configuration passed to all commands
#[derive(Debug, Clone, Copy)]
pub struct OutputConfig {
    pub json: bool,
    pub quiet: bool,
    pub verbose: bool,
}
