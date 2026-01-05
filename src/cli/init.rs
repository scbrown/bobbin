use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Overwrite existing configuration
    #[arg(long)]
    force: bool,
}

#[derive(Serialize)]
struct InitOutput {
    status: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    database: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vectors: Option<String>,
}

pub async fn run(args: InitArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let data_dir = Config::data_dir(&repo_root);
    let config_path = Config::config_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);

    // Check if already initialized
    if config_path.exists() && !args.force {
        if output.json {
            let json_output = InitOutput {
                status: "already_initialized".to_string(),
                path: data_dir.display().to_string(),
                config: Some(config_path.display().to_string()),
                database: Some(db_path.display().to_string()),
                vectors: Some(lance_path.display().to_string()),
            };
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            bail!(
                "Bobbin already initialized in {}. Use --force to reinitialize.",
                data_dir.display()
            );
        }
        return Ok(());
    }

    // Create data directory
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;

    // Create default config
    let config = Config::default();
    config.save(&config_path)?;

    if output.verbose && !output.quiet && !output.json {
        println!("  Creating config: {}", config_path.display());
    }

    // Initialize SQLite database with schema
    if args.force && db_path.exists() {
        std::fs::remove_file(&db_path).with_context(|| {
            format!("Failed to remove existing database: {}", db_path.display())
        })?;
    }
    let _metadata_store = MetadataStore::open(&db_path).with_context(|| {
        format!(
            "Failed to initialize SQLite database: {}",
            db_path.display()
        )
    })?;

    if output.verbose && !output.quiet && !output.json {
        println!("  Creating database: {}", db_path.display());
    }

    // Initialize LanceDB vector store
    if args.force && lance_path.exists() {
        std::fs::remove_dir_all(&lance_path).with_context(|| {
            format!(
                "Failed to remove existing vector store: {}",
                lance_path.display()
            )
        })?;
    }
    let _vector_store = VectorStore::open(&lance_path)
        .await
        .with_context(|| format!("Failed to initialize LanceDB: {}", lance_path.display()))?;

    if output.verbose && !output.quiet && !output.json {
        println!("  Creating vector store: {}", lance_path.display());
    }

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

            if output.verbose && !output.quiet && !output.json {
                println!("  Updated .gitignore");
            }
        }
    }

    if output.json {
        let json_output = InitOutput {
            status: "initialized".to_string(),
            path: data_dir.display().to_string(),
            config: Some(config_path.display().to_string()),
            database: Some(db_path.display().to_string()),
            vectors: Some(lance_path.display().to_string()),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        println!(
            "{} Bobbin initialized in {}",
            "✓".green(),
            data_dir.display()
        );
        println!("  Config:   {}", config_path.display());
        println!("  Database: {}", db_path.display());
        println!("  Vectors:  {}", lance_path.display());
        println!("\nNext steps:");
        println!("  {} to build the index", "bobbin index".cyan());
        println!("  {} to search code", "bobbin search <query>".cyan());
    }

    Ok(())
}
