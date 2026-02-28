use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;

use super::context::CliContentMode;
use super::OutputConfig;
use crate::access::RepoFilter;
use crate::config::Config;
use crate::index::{Embedder, GitAnalyzer};
use crate::index::git::DiffSpec;
use crate::search::context::{
    BridgeMode, ContentMode, ContextAssembler, ContextBundle, ContextConfig, FileRelevance,
};
use crate::index::git::DiffFile;
use crate::search::review::map_diff_to_chunks;
use crate::storage::{MetadataStore, VectorStore};
use std::collections::HashMap;

#[derive(Args)]
pub struct ReviewArgs {
    /// Commit range (e.g., HEAD~3..HEAD)
    #[arg(value_name = "RANGE")]
    range: Option<String>,

    /// Compare branch against main
    #[arg(long, short = 'b')]
    branch: Option<String>,

    /// Only staged changes
    #[arg(long)]
    staged: bool,

    /// Maximum lines of context to include
    #[arg(long, default_value = "500")]
    budget: usize,

    /// Coupling expansion depth (0 = no coupling)
    #[arg(long, short = 'd', default_value = "1")]
    depth: u32,

    /// Content mode: full, preview, none
    #[arg(long, short = 'c')]
    content: Option<CliContentMode>,

    /// Filter coupled files to specific repository
    #[arg(long, short = 'r')]
    repo: Option<String>,

    /// Directory to search in
    #[arg(default_value = ".")]
    path: PathBuf,
}

pub async fn run(args: ReviewArgs, output: OutputConfig) -> Result<()> {
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

    let embedder = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    // Determine diff spec from args
    let diff_spec = if let Some(ref branch) = args.branch {
        DiffSpec::Branch(branch.clone())
    } else if args.staged {
        DiffSpec::Staged
    } else if let Some(ref range) = args.range {
        DiffSpec::Range(range.clone())
    } else {
        DiffSpec::Unstaged
    };

    // Get diff files
    let git = GitAnalyzer::new(&repo_root).context("Failed to initialize git analyzer")?;
    let diff_files = git
        .get_diff_files(&diff_spec)
        .context("Failed to get diff files")?;

    if diff_files.is_empty() {
        if output.json {
            println!(r#"{{"error": "no_changes", "message": "No changes found for the specified diff."}}"#);
        } else if !output.quiet {
            println!("{} No changes found.", "!".yellow());
        }
        return Ok(());
    }

    // Map diff to seed chunks
    let seeds = map_diff_to_chunks(&diff_files, &vector_store, args.repo.as_deref())
        .await
        .context("Failed to map diff to chunks")?;

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
        max_coupled: 3,
        coupling_threshold: 0.1,
        semantic_weight: config.search.semantic_weight,
        content_mode,
        search_limit: 20,
        doc_demotion: config.search.doc_demotion,
        recency_half_life_days: config.search.recency_half_life_days,
        recency_weight: config.search.recency_weight,
        rrf_k: config.search.rrf_k,
        bridge_mode: BridgeMode::default(),
        bridge_boost_factor: 0.3,
    };

    // Build description of the diff for the query field
    let diff_description = describe_diff(&diff_spec, &args.branch);

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    let mut bundle = assembler
        .assemble_from_seeds(&diff_description, seeds, args.repo.as_deref())
        .await
        .context("Context assembly failed")?;

    // Apply role-based access filtering
    let access_filter = RepoFilter::from_config(&config.access, &output.role);
    bundle.files.retain(|f| access_filter.is_allowed(RepoFilter::repo_from_path(&f.path)));

    if output.json {
        print_json_output(&bundle, &diff_files)?;
    } else if !output.quiet {
        print_human_output(&bundle, &diff_files, &diff_description);
    }

    Ok(())
}

fn describe_diff(spec: &DiffSpec, branch: &Option<String>) -> String {
    match spec {
        DiffSpec::Unstaged => "unstaged changes".to_string(),
        DiffSpec::Staged => "staged changes".to_string(),
        DiffSpec::Branch(_) => format!(
            "branch: {}",
            branch.as_deref().unwrap_or("unknown")
        ),
        DiffSpec::Range(range) => format!("range: {}", range),
    }
}

fn print_json_output(bundle: &ContextBundle, diff_files: &[DiffFile]) -> Result<()> {
    #[derive(serde::Serialize)]
    struct ReviewOutput<'a> {
        #[serde(flatten)]
        bundle: &'a ContextBundle,
        changed_files: Vec<ChangedFileInfo>,
    }

    #[derive(serde::Serialize)]
    struct ChangedFileInfo {
        path: String,
        status: String,
        added_lines: usize,
        removed_lines: usize,
    }

    let output = ReviewOutput {
        bundle,
        changed_files: diff_files
            .iter()
            .map(|f| ChangedFileInfo {
                path: f.path.clone(),
                status: f.status.to_string(),
                added_lines: f.added_lines.len(),
                removed_lines: f.removed_lines.len(),
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_human_output(bundle: &ContextBundle, diff_files: &[DiffFile], description: &str) {
    // Build a lookup of changed files
    let changed: HashMap<&str, &DiffFile> = diff_files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    if bundle.files.is_empty() {
        println!(
            "{} No context found for: {}",
            "!".yellow(),
            description.cyan()
        );
        return;
    }

    println!(
        "{} Review context for {} changed files ({})",
        "✓".green(),
        diff_files.len(),
        description.cyan()
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
        let annotation = if let Some(diff) = changed.get(file.path.as_str()) {
            format!(
                "changed: +{} -{}",
                diff.added_lines.len(),
                diff.removed_lines.len()
            )
        } else {
            match file.relevance {
                FileRelevance::Direct => format!("direct, score: {:.4}", file.score),
                FileRelevance::Coupled => {
                    format!(
                        "coupled via git, score: {:.2}",
                        file.score
                    )
                }
                FileRelevance::Bridged => {
                    format!(
                        "bridged via doc provenance, score: {:.2}",
                        file.score
                    )
                }
            }
        };

        println!(
            "--- {} [{}] ---",
            file.path.blue(),
            annotation.dimmed()
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
    fn test_describe_diff_unstaged() {
        assert_eq!(
            describe_diff(&DiffSpec::Unstaged, &None),
            "unstaged changes"
        );
    }

    #[test]
    fn test_describe_diff_staged() {
        assert_eq!(
            describe_diff(&DiffSpec::Staged, &None),
            "staged changes"
        );
    }

    #[test]
    fn test_describe_diff_branch() {
        let branch = Some("feature/auth".to_string());
        assert_eq!(
            describe_diff(&DiffSpec::Branch("feature/auth".to_string()), &branch),
            "branch: feature/auth"
        );
    }

    #[test]
    fn test_describe_diff_range() {
        assert_eq!(
            describe_diff(&DiffSpec::Range("HEAD~3..HEAD".to_string()), &None),
            "range: HEAD~3..HEAD"
        );
    }
}
