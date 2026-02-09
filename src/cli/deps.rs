use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::OutputConfig;
use crate::config::Config;
use crate::storage::VectorStore;

#[derive(Args)]
pub struct DepsArgs {
    /// File to show dependencies for
    file: PathBuf,

    /// Show reverse dependencies (files that import this file)
    #[arg(long, short = 'r')]
    reverse: bool,

    /// Show both directions (imports and dependents)
    #[arg(long, short = 'b')]
    both: bool,
}

#[derive(Serialize)]
struct DepsOutput {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    imports: Option<Vec<DepEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependents: Option<Vec<DepEntry>>,
}

#[derive(Serialize)]
struct DepEntry {
    specifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_file: Option<String>,
}

pub async fn run(args: DepsArgs, output: OutputConfig) -> Result<()> {
    let file_path = args
        .file
        .canonicalize()
        .with_context(|| format!("File not found: {}", args.file.display()))?;

    let repo_root = find_repo_root(&file_path)?;
    let lance_path = Config::lance_path(&repo_root);

    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    let rel_path = file_path
        .strip_prefix(&repo_root)
        .context("File is not inside the repository")?
        .to_string_lossy()
        .to_string();

    let show_imports = !args.reverse || args.both;
    let show_dependents = args.reverse || args.both;

    let imports = if show_imports {
        Some(vector_store.get_dependencies(&rel_path).await?)
    } else {
        None
    };

    let dependents = if show_dependents {
        Some(vector_store.get_dependents(&rel_path).await?)
    } else {
        None
    };

    if output.json {
        let json_output = DepsOutput {
            file: rel_path,
            imports: imports.map(|imps| {
                imps.into_iter()
                    .map(|e| DepEntry {
                        specifier: e.import_statement,
                        resolved_path: if e.resolved { Some(e.file_b) } else { None },
                        source_file: None,
                    })
                    .collect()
            }),
            dependents: dependents.map(|deps| {
                deps.into_iter()
                    .map(|e| DepEntry {
                        specifier: e.import_statement,
                        resolved_path: None,
                        source_file: Some(e.file_a),
                    })
                    .collect()
            }),
        };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        if show_imports {
            if let Some(ref imps) = imports {
                println!("Imports from {}:", rel_path.cyan());
                if imps.is_empty() {
                    println!("  No imports found");
                } else {
                    for imp in imps {
                        if imp.resolved {
                            println!("  {} → {}", imp.import_statement, imp.file_b.green());
                        } else {
                            println!("  {} {}", imp.import_statement, "(unresolved)".dimmed());
                        }
                    }
                }
            }
        }

        if show_imports && show_dependents {
            println!();
        }

        if show_dependents {
            if let Some(ref deps) = dependents {
                println!("Depended on by {}:", rel_path.cyan());
                if deps.is_empty() {
                    println!("  No dependents found");
                } else {
                    for dep in deps {
                        println!("  {} (via {})", dep.file_a.green(), dep.import_statement.dimmed());
                    }
                }
            }
        }
    }

    Ok(())
}

/// Find the repository root by looking for .bobbin directory
fn find_repo_root(start_path: &Path) -> Result<PathBuf> {
    let mut current = start_path;
    if current.is_file() {
        if let Some(p) = current.parent() {
            current = p;
        }
    }

    loop {
        if Config::config_path(current).exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(p) => current = p,
            None => break,
        }
    }
    bail!("Bobbin not initialized. Run `bobbin init` first.")
}
