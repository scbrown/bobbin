use anyhow::Result;
use clap::Args;
use colored::Colorize;

use super::OutputConfig;

#[derive(Args)]
pub struct GrepArgs {
    /// Pattern to search for (regex supported)
    pattern: String,

    /// Case insensitive search
    #[arg(long, short = 'i')]
    ignore_case: bool,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,
}

pub async fn run(args: GrepArgs, output: OutputConfig) -> Result<()> {
    if output.json {
        println!(r#"{{"status": "not_implemented", "command": "grep", "pattern": "{}"}}"#,
                 args.pattern);
    } else if !output.quiet {
        println!("{} Grep command not yet implemented", "!".yellow());
        println!("  pattern: {}", args.pattern.cyan());
        println!("  ignore_case: {}", args.ignore_case);
        println!("  limit: {}", args.limit);
    }

    // TODO: Implement keyword search
    // 1. Query SQLite FTS
    // 2. Return matched chunks

    Ok(())
}
