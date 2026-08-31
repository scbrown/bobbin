//! Tests for the reranking stage (sidecar of `rerank.rs`).
//!
//! Everything here runs with deterministic fake scorers — NO model files,
//! no network, no ONNX session. The ONNX implementation is exercised only
//! at this seam; the model-in-the-loop path is untested pending a real
//! cross-encoder (there is no download path in tests or CI).

use super::*;
use crate::types::{Chunk, ChunkType, MatchType};

fn result(id: &str, score: f32) -> SearchResult {
    SearchResult {
        chunk: Chunk {
            id: id.to_string(),
            file_path: format!("src/{id}.rs"),
            chunk_type: ChunkType::Function,
            name: Some(id.to_string()),
            start_line: 1,
            end_line: 10,
            content: format!("passage {id}"),
            language: "rust".to_string(),
            tags: String::new(),
        },
        score,
        match_type: Some(MatchType::Hybrid),
        indexed_at: None,
        repo: None,
    }
}

/// Scores each passage by a fixed map from passage text; unknown = 0.0.
struct MapScorer(std::collections::HashMap<String, f32>);

impl MapScorer {
    fn new(pairs: &[(&str, f32)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(id, s)| (format!("passage {id}"), *s))
                .collect(),
        )
    }
}

impl Reranker for MapScorer {
    fn score(&self, _query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        Ok(passages
            .iter()
            .map(|p| self.0.get(*p).copied().unwrap_or(0.0))
            .collect())
    }
}

/// Always fails — for the propagation test.
struct FailingScorer;
impl Reranker for FailingScorer {
    fn score(&self, _query: &str, _passages: &[&str]) -> Result<Vec<f32>> {
        anyhow::bail!("scorer exploded")
    }
}

/// Returns the wrong number of scores — for the length-check test.
struct ShortScorer;
impl Reranker for ShortScorer {
    fn score(&self, _query: &str, _passages: &[&str]) -> Result<Vec<f32>> {
        Ok(vec![1.0])
    }
}

// ── Config parsing ────────────────────────────────────────────────────

#[test]
fn config_absent_means_off() {
    let cfg: crate::config::SearchConfig = toml::from_str("").unwrap();
    assert!(cfg.reranker.is_none());
    // And the top-level config too.
    let full: crate::config::Config = toml::from_str("").unwrap();
    assert!(full.search.reranker.is_none());
}

#[test]
fn config_parses_with_defaults() {
    let toml_src = r#"
[search.reranker]
model_path = "/models/ce.onnx"
tokenizer_path = "/models/tokenizer.json"
"#;
    let cfg: crate::config::Config = toml::from_str(toml_src).unwrap();
    let rr = cfg.search.reranker.expect("reranker section parsed");
    assert_eq!(rr.model_path, "/models/ce.onnx");
    assert_eq!(rr.tokenizer_path, "/models/tokenizer.json");
    assert_eq!(rr.max_seq_len, 512);
    assert_eq!(rr.top_k, 50);
    assert_eq!(rr.rerank_weight, 1.0);
}

#[test]
fn config_overrides_parse() {
    let toml_src = r#"
[search.reranker]
model_path = "/m.onnx"
tokenizer_path = "/t.json"
max_seq_len = 256
top_k = 20
rerank_weight = 0.5
"#;
    let cfg: crate::config::Config = toml::from_str(toml_src).unwrap();
    let rr = cfg.search.reranker.unwrap();
    assert_eq!(rr.max_seq_len, 256);
    assert_eq!(rr.top_k, 20);
    assert_eq!(rr.rerank_weight, 0.5);
}

// ── Startup validation ────────────────────────────────────────────────

#[test]
fn validate_absent_is_ok_and_missing_paths_refuse() {
    assert!(validate_config(&None).is_ok());

    let missing = RerankerConfig {
        model_path: "/definitely/not/here.onnx".into(),
        tokenizer_path: "/nor/this.json".into(),
        ..Default::default()
    };
    let err = validate_config(&Some(missing)).unwrap_err().to_string();
    assert!(err.contains("model_path"), "got: {err}");

    // Model present but tokenizer missing still refuses.
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("ce.onnx");
    std::fs::write(&model, b"not a real model").unwrap();
    let half = RerankerConfig {
        model_path: model.to_string_lossy().into_owned(),
        tokenizer_path: "/nope/tokenizer.json".into(),
        ..Default::default()
    };
    let err = validate_config(&Some(half)).unwrap_err().to_string();
    assert!(err.contains("tokenizer_path"), "got: {err}");
}

// ── apply_rerank ordering rules ───────────────────────────────────────

#[test]
fn rerank_reorders_within_top_k_only() {
    // Fused order: a > b > c > d. Reranker prefers b within top-2.
    let results = vec![
        result("a", 0.9),
        result("b", 0.8),
        result("c", 0.7),
        result("d", 0.6),
    ];
    let scorer = MapScorer::new(&[("a", -2.0), ("b", 3.0), ("c", 100.0), ("d", 100.0)]);

    let out = apply_rerank(&scorer, "q", results, 2, 1.0).unwrap();
    let ids: Vec<&str> = out.iter().map(|r| r.chunk.id.as_str()).collect();

    // b overtakes a inside the window; c/d keep their positions AFTER the
    // window even though the scorer would love them — they were never scored.
    assert_eq!(ids, ["b", "a", "c", "d"]);
}

#[test]
fn scores_outside_top_k_are_untouched() {
    let results = vec![
        result("a", 0.9),
        result("b", 0.8),
        result("c", 0.7),
        result("d", 0.6),
    ];
    let scorer = MapScorer::new(&[("a", 1.0), ("b", 2.0)]);

    let out = apply_rerank(&scorer, "q", results, 2, 1.0).unwrap();

    let c = out.iter().find(|r| r.chunk.id == "c").unwrap();
    let d = out.iter().find(|r| r.chunk.id == "d").unwrap();
    assert_eq!(c.score, 0.7, "tail fused score must be untouched");
    assert_eq!(d.score, 0.6, "tail fused score must be untouched");

    // Head scores ARE replaced by the blended value (sigmoid range).
    let b = out.iter().find(|r| r.chunk.id == "b").unwrap();
    assert!(b.score > 0.5 && b.score < 1.0, "got {}", b.score);
}

#[test]
fn top_k_larger_than_results_scores_everything() {
    let results = vec![result("a", 0.2), result("b", 0.1)];
    let scorer = MapScorer::new(&[("a", -5.0), ("b", 5.0)]);

    let out = apply_rerank(&scorer, "q", results, 50, 1.0).unwrap();
    let ids: Vec<&str> = out.iter().map(|r| r.chunk.id.as_str()).collect();
    assert_eq!(ids, ["b", "a"]);
}

#[test]
fn empty_results_are_a_noop() {
    let scorer = MapScorer::new(&[]);
    let out = apply_rerank(&scorer, "q", vec![], 50, 1.0).unwrap();
    assert!(out.is_empty());
}

#[test]
fn weight_zero_keeps_fused_order() {
    // Reranker strongly disagrees with fusion, but weight 0.0 means the
    // (normalized) fused scores decide — order must not change.
    let results = vec![result("a", 0.9), result("b", 0.5), result("c", 0.1)];
    let scorer = MapScorer::new(&[("a", -10.0), ("b", 0.0), ("c", 10.0)]);

    let out = apply_rerank(&scorer, "q", results, 3, 0.0).unwrap();
    let ids: Vec<&str> = out.iter().map(|r| r.chunk.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn intermediate_weight_blends() {
    // a leads fusion, b leads the reranker; at weight 0.5 the blend decides.
    // fused_norm: a=1.0, b=0.0; sigmoid: a≈0, b≈1 → blended a≈0.5, b≈0.5.
    // Make b's logit big enough to win the blend.
    let results = vec![result("a", 0.9), result("b", 0.8)];
    let scorer = MapScorer::new(&[("a", -10.0), ("b", 10.0)]);

    let out = apply_rerank(&scorer, "q", results, 2, 0.6).unwrap();
    let ids: Vec<&str> = out.iter().map(|r| r.chunk.id.as_str()).collect();
    assert_eq!(ids, ["b", "a"]);
}

#[test]
fn equal_fused_scores_do_not_divide_by_zero() {
    let results = vec![result("a", 0.5), result("b", 0.5)];
    let scorer = MapScorer::new(&[("a", 1.0), ("b", 2.0)]);

    let out = apply_rerank(&scorer, "q", results, 2, 0.3).unwrap();
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|r| r.score.is_finite()));
    // With identical fused scores the reranker decides even at low weight.
    assert_eq!(out[0].chunk.id, "b");
}

#[test]
fn ties_keep_fused_order_deterministically() {
    let results = vec![result("a", 0.9), result("b", 0.8), result("c", 0.7)];
    // All passages score identically → stable sort keeps fused order.
    let scorer = MapScorer::new(&[("a", 1.0), ("b", 1.0), ("c", 1.0)]);

    let out = apply_rerank(&scorer, "q", results, 3, 1.0).unwrap();
    let ids: Vec<&str> = out.iter().map(|r| r.chunk.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

// ── Failure posture ───────────────────────────────────────────────────

#[test]
fn scorer_errors_propagate() {
    let results = vec![result("a", 0.9)];
    let err = apply_rerank(&FailingScorer, "q", results, 10, 1.0).unwrap_err();
    assert!(err.to_string().contains("scorer exploded"));
}

#[test]
fn wrong_score_count_is_an_error() {
    let results = vec![result("a", 0.9), result("b", 0.8)];
    let err = apply_rerank(&ShortScorer, "q", results, 2, 1.0).unwrap_err();
    assert!(err.to_string().contains("scores"), "got: {err}");
}

// ── for_config refusal (no model files anywhere) ─────────────────────

#[test]
fn for_config_refuses_missing_files_loudly() {
    let cfg = RerankerConfig {
        model_path: "/missing/model.onnx".into(),
        tokenizer_path: "/missing/tokenizer.json".into(),
        ..Default::default()
    };
    let err = match for_config(&cfg) {
        Ok(_) => panic!("for_config must refuse missing model files"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("does not exist"), "got: {err}");
}
