use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::cli::OutputConfig;
use crate::tags::{OntologyConfig, TagsConfig};

mod infer;
mod query;

#[cfg(test)]
mod tests;

use infer::run_infer;
use query::{run_expand, run_list, run_path, run_show, run_tree};


#[derive(Args)]
pub struct OntologyArgs {
    #[command(subcommand)]
    command: OntologyCommands,

    /// Directory containing .bobbin/ config
    #[arg(default_value = ".", global = true)]
    path: PathBuf,
}

#[derive(Subcommand)]
enum OntologyCommands {
    /// Show a tag's definition, hierarchy, and relationships
    Show(ShowArgs),
    /// Expand a tag: show all descendants in the hierarchy
    Expand(ExpandArgs),
    /// Find the path between two tags through the hierarchy
    Path(PathArgs),
    /// List all tag definitions in the ontology
    List(ListArgs),
    /// Show the full ontology tree
    Tree(TreeArgs),
    /// Infer candidate ontology concepts from git coupling communities (GH#14 D5)
    Infer(InferArgs),
}

#[derive(Args)]
struct InferArgs {
    /// Minimum coupling score for edges. Default: 0.3
    #[arg(long, default_value = "0.3")]
    threshold: f32,
    /// Minimum cluster size to propose as a concept. Default: 3
    #[arg(long, default_value = "3")]
    min_size: usize,
    /// Max concepts to print. Default: 20
    #[arg(long, default_value = "20")]
    limit: usize,
}

#[derive(Args)]
struct ShowArgs {
    /// Tag name to inspect
    tag: String,
}

#[derive(Args)]
struct ExpandArgs {
    /// Tag name to expand
    tag: String,
}

#[derive(Args)]
struct PathArgs {
    /// Source tag
    from: String,
    /// Target tag
    to: String,
}

#[derive(Args)]
struct ListArgs {
    /// Show only root tags (no parent)
    #[arg(long)]
    roots: bool,
}

#[derive(Args)]
struct TreeArgs {
    /// Root tag to start from (default: show all roots)
    root: Option<String>,
}

pub async fn run(args: OntologyArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args.path.canonicalize().unwrap_or(args.path);
    let config = load_tags_config(&repo_root);

    match args.command {
        OntologyCommands::Show(a) => run_show(&config, &a, &output),
        OntologyCommands::Expand(a) => run_expand(&config, &a, &output),
        OntologyCommands::Path(a) => run_path(&config, &a, &output),
        OntologyCommands::List(a) => run_list(&config, &a, &output),
        OntologyCommands::Tree(a) => run_tree(&config, &a, &output),
        OntologyCommands::Infer(a) => run_infer(&repo_root, &config, &a, &output),
    }
}

fn load_tags_config(repo_root: &std::path::Path) -> TagsConfig {
    let tags_path = TagsConfig::tags_path(repo_root);
    if tags_path.exists() {
        TagsConfig::load_or_default(&tags_path)
    } else {
        // Try global config
        if let Some(global_dir) = crate::config::Config::global_config_dir() {
            let global_path = global_dir.join("tags.toml");
            if global_path.exists() {
                return TagsConfig::load_or_default(&global_path);
            }
        }
        TagsConfig::default()
    }
}
