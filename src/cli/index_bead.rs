//! `bobbin index-bead <id>` — single-bead incremental reindex.
//!
//! Phase 4 of `docs/plans/beads-integration.md` (GH#52). Batch bead indexing
//! (`bobbin index --include-beads`) fetches the WHOLE corpus from Dolt to
//! discover what changed. That is cheap enough nightly and far too expensive
//! for a post-write trigger that fires on every `bd update`, which is the
//! trigger the plan wants.
//!
//! This is the fast path: fetch one bead, re-embed it only if its assembled
//! content actually changed, and leave the rest of the corpus untouched.
//!
//! ## Three ways this could have been wrong, and what stops each
//!
//! **1. Deleting the corpus.** The batch path runs `index_hashed_source`,
//! whose removal sweep drops every previously indexed key missing from the
//! fetch. Pointing that at a one-bead fetch would delete every other bead on
//! every run. `index_hashed_item` narrows the sweep to the named key instead.
//!
//! **2. Re-admitting a filtered bead.** A bead that just went `closed` is
//! exactly when a post-write trigger fires. `fetch_bead` applies the same
//! visibility rules as the batch query, so a closed bead comes back as "not
//! found" and this command REMOVES it — the same end state a batch run would
//! reach, arrived at incrementally.
//!
//! **3. Poisoning the vectors.** If the configured embedding model differs
//! from the one the index was built with, a full `bobbin index` wipes and
//! rebuilds. A single-bead run cannot do that, so rather than inserting one
//! row of incompatible vectors beside 10,000 good ones, it refuses and says to
//! run a full index.

use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use std::path::PathBuf;

use super::OutputConfig;
use crate::config::Config;
use crate::index::embedder;
use crate::index::source::{index_hashed_item, ItemOutcome};
use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};

/// The repo key bead chunks and their hashes live under — the batch path's own
/// constant, not a copy of it. A second spelling here would let the two paths
/// drift onto separate corpora, each invisible to the other's sweep.
use super::index::BEADS_HASH_REPO;

/// The source label stamped on bead rows by the batch path.
const BEADS_SOURCE_LABEL: &str = "beads";

#[derive(Args)]
pub struct IndexBeadArgs {
    /// Bead identifier to reindex (e.g. bo-abc123)
    pub bead_id: String,

    /// Restrict the lookup to one rig (default: every configured database)
    #[arg(long)]
    pub rig: Option<String>,

    /// Re-embed even when the bead's content hash is unchanged
    #[arg(long)]
    pub force: bool,

    /// Directory containing .bobbin/ config (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

#[derive(Serialize)]
struct IndexBeadOutput {
    bead_id: String,
    /// One of `indexed`, `unchanged`, `removed`, `absent`.
    status: &'static str,
    /// Chunk keys this bead could occupy, in configured-rig order.
    keys: Vec<String>,
    /// The key that was actually acted on, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
}

/// Rank outcomes so a multi-rig run reports the most significant one.
///
/// A bead id can in principle exist in more than one rig. `indexed` outranks
/// `removed` outranks `unchanged` outranks `absent`, so a run that changed
/// something never reports as if it did nothing.
fn outcome_rank(outcome: ItemOutcome) -> u8 {
    match outcome {
        ItemOutcome::Indexed => 3,
        ItemOutcome::Removed => 2,
        ItemOutcome::Unchanged => 1,
        ItemOutcome::Absent => 0,
    }
}

fn outcome_label(outcome: ItemOutcome) -> &'static str {
    match outcome {
        ItemOutcome::Indexed => "indexed",
        ItemOutcome::Unchanged => "unchanged",
        ItemOutcome::Removed => "removed",
        ItemOutcome::Absent => "absent",
    }
}

pub async fn run(args: IndexBeadArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args
        .path
        .canonicalize()
        .with_context(|| format!("Invalid path: {}", args.path.display()))?;

    let config_path = Config::config_path(&repo_root);
    if !config_path.exists() {
        bail!("{}", super::not_initialized_error(&repo_root));
    }
    let config = Config::load(&config_path).with_context(|| "Failed to load configuration")?;

    // `index-bead` is a bead-only command: unlike `bobbin index`, there is no
    // file half that could still do useful work, so an unconfigured [beads]
    // section is an error rather than a silent no-op.
    if config.beads.databases.is_empty() {
        bail!(
            "No beads databases configured. Set `databases` under [beads] in {} \
             (see docs/plans/beads-integration.md).",
            config_path.display()
        );
    }
    if let Some(ref rig) = args.rig {
        let known: Vec<&str> = config
            .beads
            .databases
            .iter()
            .map(|db| crate::index::beads::rig_of(db))
            .collect();
        if !known.contains(&rig.as_str()) {
            bail!(
                "Unknown rig `{}`. Configured rigs: {}",
                rig,
                known.join(", ")
            );
        }
    }

    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);
    let metadata_store = MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    // `embedding_model` is written by the first successful `bobbin index` and
    // is the only durable evidence that one ever ran. The lance directory is
    // NOT that evidence — `bobbin init` creates it empty, so testing for it
    // let this command sail past a never-indexed repo and fail later, inside
    // the model loader, with an error about the wrong thing.
    let current_model = config.embedding.model.as_str();
    match metadata_store.get_meta("embedding_model")? {
        None => bail!(
            "No index yet at {}. Run `bobbin index --include-beads` once before \
             using index-bead.",
            lance_path.display()
        ),
        // Refuse rather than mix embedding spaces — see the module docs.
        Some(stored) if stored != current_model => bail!(
            "Embedding model changed ({} -> {}). A single-bead reindex cannot \
             migrate the index; run `bobbin index --force --include-beads`.",
            stored,
            current_model
        ),
        Some(_) => {}
    }

    let model_dir = Config::model_cache_dir()?;
    embedder::ensure_model_for_config(&model_dir, &config.embedding)
        .await
        .context("Failed to ensure embedding model is available")?;
    let embedding_dim = embedder::resolve_dimension(&config.embedding)?;
    let mut vector_store = VectorStore::open_with_dim(&lance_path, embedding_dim as i32)
        .await
        .context("Failed to open vector store")?;
    let embed = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    let mut beads_config = config.beads.clone();
    beads_config.enabled = true;

    let rig = args.rig.as_deref();
    let keys = crate::index::beads::bead_file_paths(&beads_config, &args.bead_id, rig);
    let found = crate::index::beads::fetch_bead(&beads_config, &args.bead_id, rig).await?;

    // Every candidate key is visited, not just the ones that came back: a key
    // with no chunk is how a closed or relabelled bead gets swept out.
    let mut best = ItemOutcome::Absent;
    let mut acted_key: Option<String> = None;
    for key in &keys {
        let chunk = found.iter().find(|c| &c.file_path == key);
        let outcome = index_hashed_item(
            BEADS_HASH_REPO,
            BEADS_SOURCE_LABEL,
            key,
            chunk,
            &mut vector_store,
            &metadata_store,
            &embed,
            args.force,
        )
        .await?;
        if outcome_rank(outcome) > outcome_rank(best) {
            best = outcome;
            acted_key = Some(key.clone());
        }
    }

    let status = outcome_label(best);

    if output.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&IndexBeadOutput {
                bead_id: args.bead_id.clone(),
                status,
                keys,
                key: acted_key,
            })?
        );
    } else if !output.quiet {
        match best {
            ItemOutcome::Indexed => println!(
                "{} Reindexed {} ({})",
                "✓".green(),
                args.bead_id,
                acted_key.as_deref().unwrap_or("-")
            ),
            ItemOutcome::Unchanged => {
                println!(
                    "{} {} unchanged — nothing re-embedded",
                    "=".dimmed(),
                    args.bead_id
                )
            }
            ItemOutcome::Removed => println!(
                "{} Removed {} from the index (no longer visible in Dolt)",
                "-".yellow(),
                args.bead_id
            ),
            ItemOutcome::Absent => println!(
                "{} {} is not in Dolt and not in the index — nothing to do",
                "=".dimmed(),
                args.bead_id
            ),
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "index_bead_tests.rs"]
mod tests;
