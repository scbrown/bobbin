//! `bobbin bead similar` — advisory near-duplicate detection for work items.
//!
//! bobbin already embeds the beads corpus (`src/index/beads.rs`), so a new
//! issue's title and description can be scored against the ones already
//! recorded before it is filed. This reports near-duplicates; it never refuses
//! anything (bobbin-bbe).
//!
//! **Stage-1 posture: identify and inform before anything refuses.** The exit
//! code is 0 whether or not a duplicate is found, so this can be wired into a
//! creation hook without ever blocking a write.
//!
//! **Every verdict is falsifiable.** The score, the threshold, the embedding
//! model and a corpus watermark are all reported, because a bare "this looks
//! like a duplicate of X" cannot be checked by the person reading it and
//! cannot be regression-tested later. A recommendation whose basis is not
//! recorded is an assertion.
//!
//! **"Nothing to compare against" is a distinct outcome from "no match".** An
//! empty corpus scores nothing and would otherwise render exactly like a clean
//! result — the same collapse this codebase has had to unpick elsewhere.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::OutputConfig;
use crate::config::Config;
use crate::index::Embedder;
use crate::search::SemanticSearch;
use crate::storage::{MetadataStore, VectorStore};

/// What the corpus could tell us. Kept apart deliberately: a reader who cannot
/// distinguish these three has no way to tell a safe "go ahead" from a check
/// that never ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Coverage {
    /// No beads are indexed. Nothing was compared; this is not a clean result.
    Empty,
    /// The corpus was searched and nothing cleared the threshold.
    NoMatch,
    /// At least one candidate cleared the threshold.
    Match,
}

#[derive(Serialize)]
struct Candidate {
    bead_id: String,
    title: String,
    score: f32,
    status: Option<String>,
}

#[derive(Serialize)]
struct Verdict {
    coverage: Coverage,
    candidates: Vec<Candidate>,
    /// Everything needed to reproduce or refute the verdict above.
    method: &'static str,
    model: String,
    threshold: f32,
    /// Number of bead chunks searched.
    corpus_size: usize,
    /// Digest over the sorted bead ids in the corpus. Two runs reporting the
    /// same watermark searched the same corpus; a changed watermark explains a
    /// changed verdict without anyone having to guess.
    corpus_watermark: String,
    open_only: bool,
}

/// Pull the `Status: <s> | Priority: ...` line the bead indexer appends.
///
/// Parsed rather than stored as a column because that is where the indexer
/// puts it (`build_bead_content`). Returns None when the line is absent, which
/// is treated as "unknown status" and never as "open" — guessing here would
/// silently widen the corpus the caller asked to narrow.
fn parse_status(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|l| l.strip_prefix("Status: "))
        .and_then(|rest| rest.split('|').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `beads:<rig>:<id>` -> `<id>`.
fn bead_id_from_path(path: &str) -> Option<String> {
    let mut parts = path.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("beads"), Some(_rig), Some(id)) if !id.is_empty() => Some(id.to_string()),
        _ => None,
    }
}

/// Digest over the corpus actually searched, so a verdict can be tied to it.
fn watermark(bead_ids: &mut [String]) -> String {
    use sha2::{Digest, Sha256};
    bead_ids.sort();
    let mut hasher = Sha256::new();
    for id in bead_ids.iter() {
        hasher.update(id.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", &hex::encode(hasher.finalize())[..16])
}

pub(super) async fn run_similar(
    title: &str,
    description: Option<&str>,
    threshold: f32,
    limit: usize,
    open_only: bool,
    output: &OutputConfig,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo_root = crate::cli::find_bobbin_root()
        .ok_or_else(|| anyhow::anyhow!(crate::cli::not_initialized_error(&cwd)))?;
    let config = Config::load(&Config::config_path(&repo_root)).unwrap_or_default();

    let vector_store = VectorStore::open(&Config::lance_path(&repo_root))
        .await
        .context("Failed to open the index")?;
    let metadata_store = MetadataStore::open(&Config::db_path(&repo_root))?;

    // The corpus, established before the query so an empty one is reported as
    // itself rather than inferred from an empty result set.
    let mut corpus: Vec<String> = vector_store
        .get_all_file_paths(None)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|p| bead_id_from_path(p))
        .collect();
    corpus.sort();
    corpus.dedup();
    let corpus_size = corpus.len();
    let corpus_watermark = watermark(&mut corpus);

    let model = config.embedding.model.clone();

    if corpus_size == 0 {
        let verdict = Verdict {
            coverage: Coverage::Empty,
            candidates: vec![],
            method: "semantic-cosine-v1",
            model,
            threshold,
            corpus_size,
            corpus_watermark,
            open_only,
        };
        return report(&verdict, output);
    }

    // Model consistency: comparing a fresh query embedding against vectors
    // built by a different model produces scores that mean nothing, and they
    // would look exactly like real ones.
    let current_model = config.embedding.model.as_str();
    if let Some(stored) = metadata_store.get_meta("embedding_model")? {
        if stored != current_model {
            anyhow::bail!(
                "Configured embedding model ({current_model}) differs from the indexed model \
                 ({stored}); similarity scores would not be comparable. Run `bobbin index`."
            );
        }
    }

    let query = match description {
        Some(d) if !d.trim().is_empty() => format!("{title}\n\n{d}"),
        _ => title.to_string(),
    };

    let embedder = Embedder::from_config(&config.embedding, &Config::model_cache_dir()?)
        .context("Failed to load embedding model")?;
    let mut search = SemanticSearch::new(embedder, vector_store);

    // Over-fetch: the status filter is applied in process (status is content,
    // not a column), so the pre-filter limit has to leave room.
    let raw = search
        .search_filtered(
            &query,
            (limit * 5).max(20),
            None,
            Some("language = 'beads'"),
        )
        .await
        .context("Similarity search failed")?;

    let mut candidates: Vec<Candidate> = Vec::new();
    for r in raw {
        if r.score < threshold {
            continue;
        }
        let Some(bead_id) = bead_id_from_path(&r.chunk.file_path) else {
            continue;
        };
        let status = parse_status(&r.chunk.content);
        if open_only && status.as_deref() != Some("open") {
            continue;
        }
        if candidates.iter().any(|c| c.bead_id == bead_id) {
            continue;
        }
        candidates.push(Candidate {
            bead_id,
            title: r.chunk.name.clone().unwrap_or_default(),
            score: r.score,
            status,
        });
        if candidates.len() >= limit {
            break;
        }
    }

    let verdict = Verdict {
        coverage: if candidates.is_empty() {
            Coverage::NoMatch
        } else {
            Coverage::Match
        },
        candidates,
        method: "semantic-cosine-v1",
        model,
        threshold,
        corpus_size,
        corpus_watermark,
        open_only,
    };
    report(&verdict, output)
}

fn report(verdict: &Verdict, output: &OutputConfig) -> Result<()> {
    if output.json {
        println!("{}", serde_json::to_string_pretty(verdict)?);
        return Ok(());
    }

    match verdict.coverage {
        Coverage::Empty => {
            println!("NO CORPUS — no beads are indexed, so nothing was compared.");
            println!("  This is not 'no duplicate found'. Run `bobbin index` first.");
        }
        Coverage::NoMatch => {
            println!(
                "NO NEAR-DUPLICATE — {} bead(s) searched, none at or above {:.2}.",
                verdict.corpus_size, verdict.threshold
            );
        }
        Coverage::Match => {
            println!(
                "POSSIBLE DUPLICATE — {} candidate(s) at or above {:.2}:",
                verdict.candidates.len(),
                verdict.threshold
            );
            for c in &verdict.candidates {
                println!(
                    "  {:.3}  {}  {}{}",
                    c.score,
                    c.bead_id,
                    c.title,
                    c.status
                        .as_deref()
                        .map(|s| format!("  [{s}]"))
                        .unwrap_or_default()
                );
            }
            println!("\nAdvisory only — creation is never blocked.");
        }
    }

    println!(
        "\nbasis: method={} model={} threshold={:.2} corpus={} watermark={} open_only={}",
        verdict.method,
        verdict.model,
        verdict.threshold,
        verdict.corpus_size,
        verdict.corpus_watermark,
        verdict.open_only,
    );
    Ok(())
}

#[cfg(test)]
#[path = "similar_tests.rs"]
mod tests;
