//! Optional cross-encoder reranking stage for hybrid search.
//!
//! Opt-in via `[search.reranker]` (absent = off, the default). When
//! configured, the top-K results of hybrid fusion are rescored by a
//! cross-encoder — a model that reads the (query, passage) pair jointly
//! and emits a single relevance logit — before final truncation and
//! context assembly. Ranking without it stays RRF + deterministic boosts.
//!
//! Two implementations of the [`Reranker`] seam exist: the ONNX
//! cross-encoder in `rerank_onnx.rs` (reusing the embedder's ort/tokenizer
//! machinery; model files must be user-supplied, there is no download
//! path), and deterministic test fakes in the sidecar tests, so every
//! ordering rule below is unit-tested without model files.
//!
//! Blend rule, kept deliberately simple: reranker logits are
//! sigmoid-squashed to (0,1); the K fused scores are min-max normalized to
//! [0,1]; the blended score is
//! `rerank_weight * sigmoid(logit) + (1 - rerank_weight) * fused_norm`.
//! At the default `rerank_weight = 1.0` the reranker replaces the ordering
//! within the top-K. Reordering happens strictly inside the top-K block:
//! results beyond K keep their fused scores AND their positions after it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::SearchResult;

/// Configuration for the opt-in `[search.reranker]` stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RerankerConfig {
    /// Path to the cross-encoder ONNX model file. Required — bobbin never
    /// downloads reranker models; a missing path refuses loudly at startup.
    pub model_path: String,
    /// Path to the matching tokenizer.json. Required, same posture.
    pub tokenizer_path: String,
    /// Maximum sequence length for the encoded (query, passage) pair.
    pub max_seq_len: usize,
    /// How many fused results are rescored. Results beyond this keep their
    /// fused score and order.
    pub top_k: usize,
    /// Blend between reranker score and fused score for the K candidates.
    /// 1.0 (default) = the reranker replaces the ordering within the top-K;
    /// 0.0 = fused ordering wins (the stage is a no-op).
    pub rerank_weight: f32,
}

impl Default for RerankerConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            tokenizer_path: String::new(),
            max_seq_len: 512,
            top_k: 50,
            rerank_weight: 1.0,
        }
    }
}

/// Scores (query, passage) pairs; one score per passage, higher = more
/// relevant. Raw model logits are fine — [`apply_rerank`] squashes them.
pub trait Reranker: Send + Sync {
    fn score(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>>;
}

/// Validate an optional reranker config at startup. `None` is Ok (the
/// stage is off); a configured stage with a missing model or tokenizer
/// path refuses loudly rather than degrading to unreranked results.
pub fn validate_config(cfg: &Option<RerankerConfig>) -> Result<()> {
    let Some(cfg) = cfg else { return Ok(()) };
    if cfg.model_path.is_empty() || !std::path::Path::new(&cfg.model_path).exists() {
        anyhow::bail!(
            "[search.reranker] is configured but model_path {:?} does not exist. \
             Bobbin does not download reranker models — supply a cross-encoder \
             ONNX file, or remove the [search.reranker] section.",
            cfg.model_path
        );
    }
    if cfg.tokenizer_path.is_empty() || !std::path::Path::new(&cfg.tokenizer_path).exists() {
        anyhow::bail!(
            "[search.reranker] is configured but tokenizer_path {:?} does not exist. \
             Supply the model's tokenizer.json, or remove the [search.reranker] section.",
            cfg.tokenizer_path
        );
    }
    Ok(())
}

/// Process-wide cache of loaded ONNX rerankers, keyed by model+tokenizer
/// path. HybridSearch instances are constructed per request in the server
/// paths; reloading the model each time would swamp the query.
static RERANKERS: OnceLock<Mutex<HashMap<String, Arc<dyn Reranker>>>> = OnceLock::new();

/// Get (loading and caching on first use) the ONNX reranker for a config.
/// Fails loudly if the configured files are missing or unloadable.
pub fn for_config(cfg: &RerankerConfig) -> Result<Arc<dyn Reranker>> {
    validate_config(&Some(cfg.clone()))?;
    let key = format!(
        "{}\x1f{}\x1f{}",
        cfg.model_path, cfg.tokenizer_path, cfg.max_seq_len
    );
    let cache = RERANKERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|e| anyhow::anyhow!("reranker cache lock poisoned: {e}"))?;
    if let Some(r) = cache.get(&key) {
        return Ok(r.clone());
    }
    let loaded: Arc<dyn Reranker> = Arc::new(
        super::rerank_onnx::OnnxCrossEncoder::load(cfg)
            .context("failed to load [search.reranker] cross-encoder")?,
    );
    cache.insert(key, loaded.clone());
    Ok(loaded)
}

/// Numerically stable logistic squash for raw cross-encoder logits.
fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

/// Rescore and reorder the top-K of `results`; see the module docs for the
/// blend rule. Results beyond K are untouched — same scores, same order,
/// same positions. Errors from the scorer propagate: a configured reranker
/// that cannot score is a failed search, not a silent downgrade.
pub fn apply_rerank(
    reranker: &dyn Reranker,
    query: &str,
    mut results: Vec<SearchResult>,
    top_k: usize,
    rerank_weight: f32,
) -> Result<Vec<SearchResult>> {
    let k = top_k.min(results.len());
    if k == 0 {
        return Ok(results);
    }

    let passages: Vec<&str> = results[..k]
        .iter()
        .map(|r| r.chunk.content.as_str())
        .collect();
    let raw = reranker.score(query, &passages)?;
    if raw.len() != k {
        anyhow::bail!("reranker returned {} scores for {} passages", raw.len(), k);
    }

    // Min-max normalize the K fused scores so they share the sigmoid's
    // [0,1] range; a degenerate (all-equal) head contributes a constant.
    let fused_min = results[..k]
        .iter()
        .map(|r| r.score)
        .fold(f32::INFINITY, f32::min);
    let fused_max = results[..k]
        .iter()
        .map(|r| r.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let span = fused_max - fused_min;

    let mut head: Vec<(SearchResult, f32)> = results
        .drain(..k)
        .zip(raw.iter())
        .map(|(r, &logit)| {
            let fused_norm = if span > 0.0 {
                (r.score - fused_min) / span
            } else {
                0.5
            };
            let blended = rerank_weight * sigmoid(logit) + (1.0 - rerank_weight) * fused_norm;
            (r, blended)
        })
        .collect();
    // Stable sort: ties keep fused order, so the stage is deterministic.
    head.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::with_capacity(head.len() + results.len());
    for (mut r, blended) in head {
        r.score = blended;
        out.push(r);
    }
    out.append(&mut results);
    Ok(out)
}

#[cfg(test)]
#[path = "rerank_tests.rs"]
mod tests;
