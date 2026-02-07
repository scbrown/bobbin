use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::index::Embedder;
use crate::search::context::{
    ContentMode, ContextAssembler, ContextBundle, ContextConfig, FileRelevance,
};
use crate::storage::{MetadataStore, VectorStore};

#[derive(Args)]
pub struct ContextArgs {
    /// Natural language description of the task
    query: String,

    /// Maximum lines of content to include
    #[arg(long, short = 'b', default_value = "500")]
    budget: usize,

    /// Content mode: full, preview, none
    #[arg(long, short = 'c')]
    content: Option<CliContentMode>,

    /// Coupling expansion depth (0 = no coupling)
    #[arg(long, short = 'd', default_value = "1")]
    depth: u32,

    /// Max coupled files per seed file
    #[arg(long, default_value = "3")]
    max_coupled: usize,

    /// Max initial search results
    #[arg(long, short = 'n', default_value = "20")]
    limit: usize,

    /// Min coupling score threshold
    #[arg(long, default_value = "0.1")]
    coupling_threshold: f32,

    /// Filter to specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to search in
    #[arg(default_value = ".")]
    path: PathBuf,
}

/// Content mode for CLI argument parsing
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CliContentMode {
    /// Include full chunk content
    Full,
    /// Include first 3 lines preview
    Preview,
    /// Paths/metadata only, no content
    None,
}

impl From<CliContentMode> for ContentMode {
    fn from(m: CliContentMode) -> Self {
        match m {
            CliContentMode::Full => ContentMode::Full,
            CliContentMode::Preview => ContentMode::Preview,
            CliContentMode::None => ContentMode::None,
        }
    }
}

pub async fn run(args: ContextArgs, output: OutputConfig) -> Result<()> {
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

    let config = Config::load(&config_path).with_context(|| "Failed to load configuration")?;

    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let count = vector_store.count().await?;
    if count == 0 {
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

    let metadata_store =
        MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    // Check model consistency
    let current_model = config.embedding.model.as_str();
    let stored_model = metadata_store.get_meta("embedding_model")?;
    if let Some(stored) = stored_model {
        if stored != current_model {
            bail!(
                "Configured embedding model ({}) differs from indexed model ({}). Run `bobbin index` to re-index.",
                current_model,
                stored
            );
        }
    }

    let embedder = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    // Determine content mode
    let content_mode = match args.content {
        Some(m) => m.into(),
        None => {
            if output.json {
                ContentMode::Full
            } else {
                ContentMode::Preview
            }
        }
    };

    let context_config = ContextConfig {
        budget_lines: args.budget,
        depth: args.depth,
        max_coupled: args.max_coupled,
        coupling_threshold: args.coupling_threshold,
        semantic_weight: config.search.semantic_weight,
        content_mode,
        search_limit: args.limit,
    };

    let assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let bundle = assembler
        .assemble(&args.query, args.repo.as_deref())
        .await
        .context("Context assembly failed")?;

    if output.json {
        print_json_output(&bundle)?;
    } else if !output.quiet {
        print_human_output(&bundle);
    }

    Ok(())
}

fn print_json_output(bundle: &ContextBundle) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(bundle)?);
    Ok(())
}

fn print_human_output(bundle: &ContextBundle) {
    if bundle.files.is_empty() {
        println!(
            "{} No context found for: {}",
            "!".yellow(),
            bundle.query.cyan()
        );
        return;
    }

    println!(
        "{} Context for: {}",
        "✓".green(),
        bundle.query.cyan()
    );
    println!(
        "  {} files, {} chunks ({}/{} lines)",
        bundle.summary.total_files,
        bundle.summary.total_chunks,
        bundle.budget.used_lines,
        bundle.budget.max_lines
    );
    println!();

    for file in &bundle.files {
        let relevance_info = match file.relevance {
            FileRelevance::Direct => format!("direct, score: {:.4}", file.score),
            FileRelevance::Coupled => {
                format!("coupled via {}", file.coupled_to.join(", "))
            }
        };

        println!(
            "--- {} [{}] ---",
            file.path.blue(),
            relevance_info.dimmed()
        );

        for chunk in &file.chunks {
            let name_display = chunk
                .name
                .as_ref()
                .map(|n| format!("{} ", n))
                .unwrap_or_default();

            println!(
                "  {}{}, lines {}-{}",
                name_display,
                format!("({})", chunk.chunk_type).magenta(),
                chunk.start_line,
                chunk.end_line
            );

            if let Some(ref content) = chunk.content {
                for line in content.lines() {
                    println!("  {}", line.dimmed());
                }
            }
        }

        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_mode_clap_parsing() {
        // Verify the enum variants exist and convert correctly
        assert_eq!(
            std::mem::discriminant(&ContentMode::from(CliContentMode::Full)),
            std::mem::discriminant(&ContentMode::Full)
        );
        assert_eq!(
            std::mem::discriminant(&ContentMode::from(CliContentMode::Preview)),
            std::mem::discriminant(&ContentMode::Preview)
        );
        assert_eq!(
            std::mem::discriminant(&ContentMode::from(CliContentMode::None)),
            std::mem::discriminant(&ContentMode::None)
        );
    }
}
