use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::storage::VectorStore;
use crate::tags::{validate_tag, TagRule, TagsConfig};

#[derive(Args)]
pub struct TagArgs {
    #[command(subcommand)]
    command: TagCommands,

    /// Directory containing .bobbin/ config
    #[arg(default_value = ".", global = true)]
    path: PathBuf,
}

#[derive(Subcommand)]
enum TagCommands {
    /// List tags in use
    List(ListArgs),
    /// Add a pattern-based tag rule to tags.toml
    Add(AddArgs),
    /// Remove a tag rule from tags.toml
    Remove(RemoveArgs),
}

#[derive(Args)]
struct ListArgs {
    /// Show tags for a specific file
    #[arg(long)]
    file: Option<String>,
    /// Show files with a specific tag
    #[arg(long)]
    tag: Option<String>,
    /// Show tag usage statistics
    #[arg(long)]
    stats: bool,
}

#[derive(Args)]
struct AddArgs {
    /// Glob pattern to match file paths
    pattern: String,
    /// Tags to apply (at least one)
    #[arg(required = true)]
    tags: Vec<String>,
    /// Optional repo scope
    #[arg(long)]
    repo: Option<String>,
}

#[derive(Args)]
struct RemoveArgs {
    /// Glob pattern (must match an existing rule exactly)
    pattern: String,
    /// Tag to remove from the rule
    tag: String,
}

pub async fn run(args: TagArgs, output: OutputConfig) -> Result<()> {
    match args.command {
        TagCommands::List(list_args) => run_list(args.path, list_args, output).await,
        TagCommands::Add(add_args) => run_add(args.path, add_args, output),
        TagCommands::Remove(remove_args) => run_remove(args.path, remove_args, output),
    }
}

async fn run_list(path: PathBuf, args: ListArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let lance_path = Config::lance_path(&repo_root);

    if !lance_path.exists() {
        bail!("No index found at {}. Run `bobbin index` first.", lance_path.display());
    }

    let store = VectorStore::open(&lance_path)
        .await
        .context("opening vector store")?;

    if let Some(ref file) = args.file {
        // Show tags for a specific file
        let chunks = store.get_chunks_for_file(file, None).await?;
        if chunks.is_empty() {
            if !output.quiet {
                eprintln!("No chunks found for {}", file);
            }
            return Ok(());
        }

        // Collect unique tags across all chunks of this file
        let mut tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for chunk in &chunks {
            if !chunk.tags.is_empty() {
                for t in chunk.tags.split(',') {
                    tags.insert(t.to_string());
                }
            }
        }

        if output.json {
            let json = serde_json::json!({
                "file": file,
                "tags": tags.into_iter().collect::<Vec<_>>(),
                "chunk_count": chunks.len(),
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else if tags.is_empty() {
            println!("{}: no tags", file);
        } else {
            println!("{}: {}", file, tags.into_iter().collect::<Vec<_>>().join(", "));
        }
        return Ok(());
    }

    if let Some(ref tag) = args.tag {
        // Show files with a specific tag
        let files = store.get_files_by_tag(tag).await?;
        if output.json {
            let json = serde_json::json!({
                "tag": tag,
                "files": files,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else if files.is_empty() {
            println!("No files tagged '{}'", tag);
        } else {
            println!("Files tagged '{}':", tag);
            for f in &files {
                println!("  {}", f);
            }
        }
        return Ok(());
    }

    // Default: list all tags with counts
    let tag_counts = store.get_tag_counts().await?;

    if output.json {
        let json: Vec<serde_json::Value> = tag_counts
            .iter()
            .map(|(tag, count)| serde_json::json!({"tag": tag, "count": count}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if tag_counts.is_empty() {
        println!("No tags in index. Add rules to .bobbin/tags.toml and reindex.");
    } else {
        if args.stats {
            let (tagged, untagged) = store.count_tagged_chunks().await?;
            println!("Tagged chunks: {}", tagged);
            println!("Untagged chunks: {}", untagged);
            println!();
        }
        for (tag, count) in &tag_counts {
            println!("  {:24} {:>6} chunks", tag, count);
        }
    }

    Ok(())
}

fn run_add(path: PathBuf, args: AddArgs, output: OutputConfig) -> Result<()> {
    // Validate all tags
    for tag in &args.tags {
        validate_tag(tag)?;
    }

    let repo_root = path.canonicalize().unwrap_or(path);
    let tags_path = TagsConfig::tags_path(&repo_root);
    let mut config = TagsConfig::load_or_default(&tags_path);

    // Check for duplicate rule (same pattern + repo)
    let existing = config.rules.iter_mut().find(|r| {
        r.pattern == args.pattern && r.repo == args.repo
    });

    if let Some(rule) = existing {
        // Merge tags into existing rule
        for tag in &args.tags {
            if !rule.tags.contains(tag) {
                rule.tags.push(tag.clone());
            }
        }
        rule.tags.sort();
    } else {
        let mut sorted_tags = args.tags.clone();
        sorted_tags.sort();
        config.rules.push(TagRule {
            pattern: args.pattern.clone(),
            tags: sorted_tags,
            repo: args.repo.clone(),
        });
    }

    config.save(&tags_path)?;

    if !output.quiet {
        let scope = args
            .repo
            .as_ref()
            .map(|r| format!(" (repo: {})", r))
            .unwrap_or_default();
        println!(
            "Added tags [{}] to pattern '{}'{}\nReindex to apply: bobbin index --force",
            args.tags.join(", "),
            args.pattern,
            scope,
        );
    }

    Ok(())
}

fn run_remove(path: PathBuf, args: RemoveArgs, output: OutputConfig) -> Result<()> {
    let repo_root = path.canonicalize().unwrap_or(path);
    let tags_path = TagsConfig::tags_path(&repo_root);

    if !tags_path.exists() {
        bail!("No tags.toml found at {}", tags_path.display());
    }

    let mut config = TagsConfig::load(&tags_path)?;

    let rule_idx = config
        .rules
        .iter()
        .position(|r| r.pattern == args.pattern)
        .with_context(|| format!("No rule found for pattern '{}'", args.pattern))?;

    let rule = &mut config.rules[rule_idx];
    rule.tags.retain(|t| t != &args.tag);

    if rule.tags.is_empty() {
        config.rules.remove(rule_idx);
        if !output.quiet {
            println!(
                "Removed rule for pattern '{}' (no tags remaining)\nReindex to apply: bobbin index --force",
                args.pattern
            );
        }
    } else {
        if !output.quiet {
            println!(
                "Removed tag '{}' from pattern '{}' (remaining: {})\nReindex to apply: bobbin index --force",
                args.tag,
                args.pattern,
                rule.tags.join(", "),
            );
        }
    }

    config.save(&tags_path)?;
    Ok(())
}
