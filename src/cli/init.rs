use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Overwrite existing configuration
    #[arg(long)]
    force: bool,
}

pub async fn run(args: InitArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args.path.canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let data_dir = Config::data_dir(&repo_root);
    let config_path = Config::config_path(&repo_root);

    // Check if already initialized
    if config_path.exists() && !args.force {
        if output.json {
            println!(r#"{{"status": "already_initialized", "path": "{}"}}"#,
                     data_dir.display());
        } else {
            bail!("Bobbin already initialized in {}. Use --force to reinitialize.",
                  data_dir.display());
        }
        return Ok(());
    }

    // Create data directory
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;

    // Create default config
    let config = Config::default();
    config.save(&config_path)?;

    // Add .bobbin to .gitignore if it exists
    let gitignore_path = repo_root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content.contains(".bobbin") {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            use std::io::Write;
            writeln!(file, "\n# Bobbin index data\n.bobbin/")?;
        }
    }

    if output.json {
        println!(r#"{{"status": "initialized", "path": "{}", "config": "{}"}}"#,
                 data_dir.display(), config_path.display());
    } else if !output.quiet {
        println!("{} Bobbin initialized in {}", "✓".green(), data_dir.display());
        println!("  Config: {}", config_path.display());
        println!("\nNext steps:");
        println!("  {} to build the index", "bobbin index".cyan());
        println!("  {} to search code", "bobbin search <query>".cyan());
    }

    Ok(())
}
