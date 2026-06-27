//! Personalized PageRank ranking signal (GH companion: docs/plans/ppr-ranking-signal.md).
//!
//! Quipu owns the graph algorithm (`quipu::page_rank` / `tool_project` "ppr").
//! Bobbin owns retrieval fusion: seed PPR with the top hybrid hits, then fold the
//! per-file connectivity score into ranking as a *bounded multiplier* alongside
//! recency and repo-affinity. Without the `knowledge` feature this is absent and
//! ranking degrades gracefully to today's behavior.

/// Default number of top hits used to seed Personalized PageRank.
pub const DEFAULT_SEED_K: usize = 10;
/// Default PageRank damping.
pub const DEFAULT_DAMPING: f32 = 0.85;

/// Bounded score multiplier for a result's normalized PPR score.
///
/// `weight` controls influence (0.0 = no effect). The PPR score is clamped to
/// `[0,1]` so the multiplier stays within `[1.0, 1.0 + weight]` — graph
/// connectivity can promote but never zero out a textually-relevant hit.
pub fn ppr_multiplier(ppr_score: f32, weight: f32) -> f32 {
    1.0 + weight * ppr_score.clamp(0.0, 1.0)
}

/// Compute a per-file Personalized PageRank score map over the code coupling
/// graph in Quipu, seeded by the highest-scoring candidate files.
///
/// `candidates` are `(file_path, score)` pairs for the current result set;
/// `repo` is used to build `code_module_iri`s. Returns `file_path -> normalized
/// score in [0,1]`. Best-effort: returns an empty map on any error so ranking
/// is never broken by a graph hiccup.
#[cfg(feature = "knowledge")]
pub fn compute_code_ppr(
    store: &quipu::Store,
    candidates: &[(String, f32)],
    repo: &str,
    seed_k: usize,
    damping: f32,
) -> std::collections::HashMap<String, f32> {
    use crate::knowledge::coupling::{co_changed_with_iri, code_module_iri};
    use std::collections::HashMap;

    if candidates.is_empty() {
        return HashMap::new();
    }

    // Forward map: entity IRI -> file_path, for the files we can rank.
    let mut iri_to_file: HashMap<String, String> = HashMap::new();
    for (file, _) in candidates {
        iri_to_file.insert(code_module_iri(repo, file), file.clone());
    }

    // Seeds: the top-k candidate files by current score.
    let mut sorted: Vec<&(String, f32)> = candidates.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let seeds: Vec<String> = sorted
        .iter()
        .take(seed_k.max(1))
        .map(|(file, _)| code_module_iri(repo, file))
        .collect();

    let input = serde_json::json!({
        "algorithm": "ppr",
        "predicate": co_changed_with_iri(),
        "seeds": seeds,
        "damping": damping,
        "limit": candidates.len().max(50),
    });

    let out = match quipu::tool_project(store, &input) {
        Ok(o) => o,
        Err(_) => return HashMap::new(),
    };

    let mut scores: HashMap<String, f32> = HashMap::new();
    if let Some(arr) = out.get("results").and_then(|v| v.as_array()) {
        for r in arr {
            if let (Some(iri), Some(s)) = (
                r.get("entity").and_then(|v| v.as_str()),
                r.get("score").and_then(|v| v.as_f64()),
            ) {
                if let Some(file) = iri_to_file.get(iri) {
                    scores.insert(file.clone(), s as f32);
                }
            }
        }
    }

    // Normalize to [0,1] by max so the bounded multiplier is comparable across
    // queries regardless of graph size.
    let max = scores.values().copied().fold(0.0f32, f32::max);
    if max > 0.0 {
        for v in scores.values_mut() {
            *v /= max;
        }
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppr_multiplier_zero_weight_is_identity() {
        assert_eq!(ppr_multiplier(1.0, 0.0), 1.0);
        assert_eq!(ppr_multiplier(0.5, 0.0), 1.0);
    }

    #[test]
    fn test_ppr_multiplier_bounded() {
        // weight=0.3, full score → at most 1.3
        assert!((ppr_multiplier(1.0, 0.3) - 1.3).abs() < f32::EPSILON);
        // clamps out-of-range scores
        assert!((ppr_multiplier(5.0, 0.3) - 1.3).abs() < f32::EPSILON);
        assert!((ppr_multiplier(-1.0, 0.3) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_ppr_multiplier_monotonic() {
        let w = 0.4;
        assert!(ppr_multiplier(0.2, w) < ppr_multiplier(0.8, w));
    }
}
