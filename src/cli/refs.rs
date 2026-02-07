use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::analysis::refs::RefAnalyzer;
use crate::config::Config;
use crate::storage::VectorStore;

#[derive(Args)]
pub struct RefsArgs {
    #[command(subcommand)]
    command: RefsCommand,

    /// Directory to search in (defaults to current directory)
    #[arg(long, default_value = ".", global = true)]
    path: PathBuf,

    /// Filter results to a specific repository
    #[arg(long, short = 'r', global = true)]
    repo: Option<String>,
}

#[derive(Subcommand)]
enum RefsCommand {
    /// Find references to a symbol (definition + usages)
    Find(FindArgs),

    /// List all symbols defined in a file
    Symbols(SymbolsArgs),
}

#[derive(Args)]
struct FindArgs {
    /// Symbol name to find references for
    symbol: String,

    /// Filter by symbol type (function, struct, trait, etc.)
    #[arg(long, short = 't')]
    r#type: Option<String>,

    /// Maximum number of usage results
    #[arg(long, short = 'n', default_value = "20")]
    limit: usize,
}

#[derive(Args)]
struct SymbolsArgs {
    /// File path to list symbols for
    file: PathBuf,
}

#[derive(Serialize)]
struct FindOutput {
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<DefinitionOutput>,
    usage_count: usize,
    usages: Vec<UsageOutput>,
}

#[derive(Serialize)]
struct DefinitionOutput {
    name: String,
    chunk_type: String,
    file_path: String,
    start_line: u32,
    end_line: u32,
    signature: String,
}

#[derive(Serialize)]
struct UsageOutput {
    file_path: String,
    line: u32,
    context: String,
}

#[derive(Serialize)]
struct SymbolsOutput {
    file: String,
    count: usize,
    symbols: Vec<SymbolOutput>,
}

#[derive(Serialize)]
struct SymbolOutput {
    name: String,
    chunk_type: String,
    start_line: u32,
    end_line: u32,
    signature: String,
}

pub async fn run(args: RefsArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!(
            "Bobbin not initialized in {}. Run `bobbin init` first.",
            repo_root.display()
        );
    }

    let lance_path = Config::lance_path(&repo_root);
    let mut vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let stats = vector_store.get_stats(None).await?;
    if stats.total_chunks == 0 {
        if output.json {
            println!(
                r#"{{"error": "empty_index", "message": "No indexed content. Run `bobbin index` first."}}"#
            );
        } else if !output.quiet {
            println!(
                "{} No indexed content. Run `bobbin index` first.",
                "!".yellow()
            );
        }
        return Ok(());
    }

    match args.command {
        RefsCommand::Find(find_args) => {
            run_find(&mut vector_store, find_args, args.repo.as_deref(), &output).await
        }
        RefsCommand::Symbols(sym_args) => {
            run_symbols(&mut vector_store, sym_args, &repo_root, args.repo.as_deref(), &output).await
        }
    }
}

async fn run_find(
    vector_store: &mut VectorStore,
    args: FindArgs,
    repo: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    let type_filter = args.r#type.as_deref();

    let mut analyzer = RefAnalyzer::new(vector_store);
    let refs = analyzer
        .find_refs(&args.symbol, type_filter, args.limit, repo)
        .await?;

    if output.json {
        let json_output = FindOutput {
            symbol: args.symbol.clone(),
            r#type: args.r#type.clone(),
            definition: refs.definition.map(|d| DefinitionOutput {
                name: d.name,
                chunk_type: d.chunk_type.to_string(),
                file_path: d.file_path,
                start_line: d.start_line,
                end_line: d.end_line,
                signature: d.signature,
            }),
            usage_count: refs.usages.len(),
            usages: refs
                .usages
                .iter()
                .map(|u| UsageOutput {
                    file_path: u.file_path.clone(),
                    line: u.line,
                    context: u.context.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        // Definition
        if let Some(ref def) = refs.definition {
            println!(
                "{} {} {} defined at {}:{}",
                "✓".green(),
                def.chunk_type.to_string().magenta(),
                def.name.cyan().bold(),
                def.file_path.blue(),
                def.start_line,
            );
            println!("  {}", def.signature.dimmed());
        } else {
            println!(
                "{} No definition found for: {}",
                "!".yellow(),
                args.symbol.cyan()
            );
        }

        // Usages
        if refs.usages.is_empty() {
            println!("\n  No usages found.");
        } else {
            println!(
                "\n{} {} usage{}:",
                "→".blue(),
                refs.usages.len(),
                if refs.usages.len() == 1 { "" } else { "s" }
            );
            for usage in &refs.usages {
                println!(
                    "  {}:{} {}",
                    usage.file_path.blue(),
                    usage.line.to_string().dimmed(),
                    usage.context
                );
            }
        }
    }

    Ok(())
}

async fn run_symbols(
    vector_store: &mut VectorStore,
    args: SymbolsArgs,
    repo_root: &std::path::Path,
    repo: Option<&str>,
    output: &OutputConfig,
) -> Result<()> {
    // Resolve file path relative to repo root
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("File not found: {}", args.file.display()))?;

    let rel_path = file_path
        .strip_prefix(repo_root)
        .context("File is not inside the repository")?
        .to_string_lossy()
        .to_string();

    let analyzer = RefAnalyzer::new(vector_store);
    let file_symbols = analyzer.list_symbols(&rel_path, repo).await?;

    if output.json {
        let json_output = SymbolsOutput {
            file: file_symbols.path,
            count: file_symbols.symbols.len(),
            symbols: file_symbols
                .symbols
                .iter()
                .map(|s| SymbolOutput {
                    name: s.name.clone(),
                    chunk_type: s.chunk_type.to_string(),
                    start_line: s.start_line,
                    end_line: s.end_line,
                    signature: s.signature.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else if !output.quiet {
        if file_symbols.symbols.is_empty() {
            println!(
                "{} No symbols found in: {}",
                "!".yellow(),
                rel_path.cyan()
            );
        } else {
            println!(
                "Symbols in {}:",
                rel_path.cyan()
            );
            for symbol in &file_symbols.symbols {
                println!(
                    "  {} {} (lines {}-{})",
                    symbol.chunk_type.to_string().magenta(),
                    symbol.name.bold(),
                    symbol.start_line,
                    symbol.end_line,
                );
                if output.verbose {
                    println!("    {}", symbol.signature.dimmed());
                }
            }
            println!(
                "\n{} {} symbol{}",
                "✓".green(),
                file_symbols.symbols.len(),
                if file_symbols.symbols.len() == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

