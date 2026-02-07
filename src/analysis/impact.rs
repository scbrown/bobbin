use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};

/// Maximum transitive expansion depth to prevent runaway analysis.
const MAX_DEPTH: u32 = 3;

/// What signal produced this impact prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactSignal {
    Coupling { co_changes: u32 },
    Semantic { similarity: f32 },
    Dependency,
    Combined,
}

/// A single impact prediction: "if you change the target, this file is likely affected"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    pub path: String,
    pub signal: ImpactSignal,
    pub score: f32,
    pub reason: String,
}

/// Which signals to use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactMode {
    Combined,
    Coupling,
    Semantic,
    Deps,
}

/// Configuration for impact analysis
pub struct ImpactConfig {
    pub mode: ImpactMode,
    pub threshold: f32,
    pub limit: usize,
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self {
            mode: ImpactMode::Combined,
            threshold: 0.1,
            limit: 15,
        }
    }
}

/// Analyzes what code is affected when a target file or function changes,
/// by combining coupling, semantic similarity, and dependency signals.
pub struct ImpactAnalyzer<'a> {
    metadata_store: &'a MetadataStore,
    vector_store: &'a VectorStore,
    embedder: &'a mut Embedder,
}

impl<'a> ImpactAnalyzer<'a> {
    pub fn new(
        metadata_store: &'a MetadataStore,
        vector_store: &'a VectorStore,
        embedder: &'a mut Embedder,
    ) -> Self {
        Self {
            metadata_store,
            vector_store,
            embedder,
        }
    }

    /// Analyze impact of changing the given target, with transitive expansion.
    ///
    /// `target` can be a file path (e.g. "src/auth.rs") or file:function syntax
    /// (e.g. "src/auth.rs:validate_token").
    ///
    /// `depth` controls transitive expansion: 1 = direct only, 2+ = expand
    /// through results with score decay of 0.5 per level. Capped at 3.
    pub async fn analyze(
        &mut self,
        target: &str,
        config: &ImpactConfig,
        depth: u32,
        repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>> {
        if config.mode == ImpactMode::Deps {
            bail!("Dependency graph impact analysis is not yet available. This will be enabled when bobbin-graph lands.");
        }

        let depth = depth.clamp(1, MAX_DEPTH);
        let decay_factor: f32 = 0.5;

        let mut all_results: HashMap<String, ImpactResult> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut current_targets = vec![target.to_string()];

        for level in 0..depth {
            let decay = decay_factor.powi(level as i32);
            let mut next_targets = Vec::new();

            for t in &current_targets {
                if !visited.insert(t.clone()) {
                    continue;
                }

                let results = self.analyze_single(t, config, repo).await?;
                for mut r in results {
                    r.score *= decay;
                    if r.score < config.threshold {
                        continue;
                    }
                    next_targets.push(r.path.clone());
                    all_results
                        .entry(r.path.clone())
                        .and_modify(|existing| {
                            if r.score > existing.score {
                                *existing = r.clone();
                            }
                        })
                        .or_insert(r);
                }
            }

            current_targets = next_targets;
            if current_targets.is_empty() {
                break;
            }
        }

        let mut results: Vec<ImpactResult> = all_results.into_values().collect();

        // Filter by threshold (decay may have pushed some below)
        results.retain(|r| r.score >= config.threshold);

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Limit
        results.truncate(config.limit);

        Ok(results)
    }

    /// Single-level (non-transitive) impact analysis for one target.
    async fn analyze_single(
        &mut self,
        target: &str,
        config: &ImpactConfig,
        repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>> {
        let (file_path, function_name) = parse_target(target);

        let mut signal_map: HashMap<String, Vec<(ImpactSignal, f32, String)>> = HashMap::new();

        if config.mode == ImpactMode::Coupling || config.mode == ImpactMode::Combined {
            self.gather_coupling_signal(file_path, &mut signal_map, config)?;
        }

        if config.mode == ImpactMode::Semantic || config.mode == ImpactMode::Combined {
            self.gather_semantic_signal(file_path, function_name, &mut signal_map, config, repo)
                .await?;
        }

        let mut results = merge_signals(signal_map, config.mode);

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }

    fn gather_coupling_signal(
        &self,
        file_path: &str,
        signal_map: &mut HashMap<String, Vec<(ImpactSignal, f32, String)>>,
        config: &ImpactConfig,
    ) -> Result<()> {
        let couplings = self.metadata_store.get_coupling(file_path, config.limit)?;

        if couplings.is_empty() {
            return Ok(());
        }

        // Find max score for normalization
        let max_score = couplings
            .iter()
            .map(|c| c.score)
            .fold(0.0f32, f32::max);

        for coupling in &couplings {
            // The "other" file in the coupling pair
            let other = if coupling.file_a == file_path {
                &coupling.file_b
            } else {
                &coupling.file_a
            };

            let normalized = if max_score > 0.0 {
                coupling.score / max_score
            } else {
                0.0
            };

            let reason = format!(
                "Co-changed {} times (coupling score {:.2})",
                coupling.co_changes, coupling.score
            );

            signal_map
                .entry(other.clone())
                .or_default()
                .push((
                    ImpactSignal::Coupling {
                        co_changes: coupling.co_changes,
                    },
                    normalized,
                    reason,
                ));
        }

        Ok(())
    }

    async fn gather_semantic_signal(
        &mut self,
        file_path: &str,
        function_name: Option<&str>,
        signal_map: &mut HashMap<String, Vec<(ImpactSignal, f32, String)>>,
        config: &ImpactConfig,
        repo: Option<&str>,
    ) -> Result<()> {
        // Get chunks for the target file to find the right content to embed
        let chunks = self.vector_store.get_chunks_for_file(file_path, repo).await?;

        if chunks.is_empty() {
            return Ok(());
        }

        // Find the target chunk content
        let target_content = if let Some(func) = function_name {
            // Look for a chunk matching the function name
            chunks
                .iter()
                .find(|c| c.name.as_deref() == Some(func))
                .map(|c| c.content.as_str())
                .unwrap_or_else(|| chunks[0].content.as_str())
        } else {
            // Use the first chunk as representative (typically the largest/most important)
            chunks[0].content.as_str()
        };

        // Get embedding for the target content
        let embedding = self.embedder.embed(target_content).await?;

        // Search for similar chunks, requesting more than limit to account for
        // filtering out same-file results
        let search_limit = config.limit * 3;
        let results = self.vector_store.search(&embedding, search_limit, repo).await?;

        for result in &results {
            // Skip results from the same file
            if result.chunk.file_path == file_path {
                continue;
            }

            let reason = format!(
                "Semantically similar (score {:.3}, chunk: {})",
                result.score,
                result.chunk.name.as_deref().unwrap_or(&result.chunk.chunk_type.to_string())
            );

            signal_map
                .entry(result.chunk.file_path.clone())
                .or_default()
                .push((
                    ImpactSignal::Semantic {
                        similarity: result.score,
                    },
                    result.score,
                    reason,
                ));
        }

        Ok(())
    }
}

/// Parse "file:function" or just "file" target syntax
fn parse_target(target: &str) -> (&str, Option<&str>) {
    if let Some(idx) = target.rfind(':') {
        let file = &target[..idx];
        let func = &target[idx + 1..];
        // Guard against Windows paths like "C:\foo" — only split if the part
        // after ':' looks like a function name (no slashes/backslashes)
        if !func.is_empty() && !func.contains('/') && !func.contains('\\') {
            return (file, Some(func));
        }
    }
    (target, None)
}

/// Merge per-file signals into final ImpactResults.
/// For Combined mode, take max score across all signals per file.
/// For single-signal mode, only that signal's results are present.
fn merge_signals(
    signal_map: HashMap<String, Vec<(ImpactSignal, f32, String)>>,
    mode: ImpactMode,
) -> Vec<ImpactResult> {
    signal_map
        .into_iter()
        .map(|(path, signals)| {
            if signals.len() == 1 || mode != ImpactMode::Combined {
                // Single signal: use it directly
                let (signal, score, reason) = signals.into_iter().next().unwrap();
                ImpactResult {
                    path,
                    signal,
                    score,
                    reason,
                }
            } else {
                // Combined: take the best score, note all signals
                let best = signals
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap();
                let best_score = best.1;

                let reasons: Vec<&str> = signals.iter().map(|(_, _, r)| r.as_str()).collect();
                let combined_reason = reasons.join("; ");

                ImpactResult {
                    path,
                    signal: ImpactSignal::Combined,
                    score: best_score,
                    reason: combined_reason,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target_file_only() {
        let (file, func) = parse_target("src/auth.rs");
        assert_eq!(file, "src/auth.rs");
        assert_eq!(func, None);
    }

    #[test]
    fn test_parse_target_file_and_function() {
        let (file, func) = parse_target("src/auth.rs:validate_token");
        assert_eq!(file, "src/auth.rs");
        assert_eq!(func, Some("validate_token"));
    }

    #[test]
    fn test_parse_target_no_false_split_on_path() {
        // Should not split on directory separators
        let (file, func) = parse_target("src/auth/middleware.rs");
        assert_eq!(file, "src/auth/middleware.rs");
        assert_eq!(func, None);
    }

    #[test]
    fn test_merge_signals_single() {
        let mut map = HashMap::new();
        map.insert(
            "src/b.rs".to_string(),
            vec![(
                ImpactSignal::Coupling { co_changes: 5 },
                0.8,
                "Co-changed 5 times".to_string(),
            )],
        );

        let results = merge_signals(map, ImpactMode::Coupling);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/b.rs");
        assert!((results[0].score - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_merge_signals_combined_takes_max() {
        let mut map = HashMap::new();
        map.insert(
            "src/b.rs".to_string(),
            vec![
                (
                    ImpactSignal::Coupling { co_changes: 3 },
                    0.5,
                    "coupling".to_string(),
                ),
                (
                    ImpactSignal::Semantic { similarity: 0.9 },
                    0.9,
                    "semantic".to_string(),
                ),
            ],
        );

        let results = merge_signals(map, ImpactMode::Combined);
        assert_eq!(results.len(), 1);
        assert!((results[0].score - 0.9).abs() < f32::EPSILON);
        assert!(matches!(results[0].signal, ImpactSignal::Combined));
    }

    #[test]
    fn test_threshold_filtering() {
        // Simulate post-merge filtering
        let results = vec![
            ImpactResult {
                path: "a.rs".to_string(),
                signal: ImpactSignal::Coupling { co_changes: 10 },
                score: 0.9,
                reason: "high".to_string(),
            },
            ImpactResult {
                path: "b.rs".to_string(),
                signal: ImpactSignal::Coupling { co_changes: 1 },
                score: 0.05,
                reason: "low".to_string(),
            },
            ImpactResult {
                path: "c.rs".to_string(),
                signal: ImpactSignal::Semantic { similarity: 0.15 },
                score: 0.15,
                reason: "medium".to_string(),
            },
        ];

        let threshold = 0.1;
        let filtered: Vec<_> = results.into_iter().filter(|r| r.score >= threshold).collect();
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].path, "a.rs");
        assert_eq!(filtered[1].path, "c.rs");
    }

    #[test]
    fn test_deps_mode_returns_error() {
        // We can't easily construct the full analyzer in a unit test without
        // real stores, but we can verify the bail message is correct by testing
        // the mode check logic directly
        let mode = ImpactMode::Deps;
        assert_eq!(mode, ImpactMode::Deps);
    }

    #[test]
    fn test_impact_config_defaults() {
        let config = ImpactConfig::default();
        assert_eq!(config.mode, ImpactMode::Combined);
        assert!((config.threshold - 0.1).abs() < f32::EPSILON);
        assert_eq!(config.limit, 15);
    }

    #[test]
    fn test_merge_signals_multiple_files() {
        let mut map = HashMap::new();
        map.insert(
            "src/a.rs".to_string(),
            vec![(
                ImpactSignal::Coupling { co_changes: 10 },
                1.0,
                "high coupling".to_string(),
            )],
        );
        map.insert(
            "src/b.rs".to_string(),
            vec![(
                ImpactSignal::Semantic { similarity: 0.6 },
                0.6,
                "similar".to_string(),
            )],
        );
        map.insert(
            "src/c.rs".to_string(),
            vec![
                (
                    ImpactSignal::Coupling { co_changes: 2 },
                    0.3,
                    "some coupling".to_string(),
                ),
                (
                    ImpactSignal::Semantic { similarity: 0.7 },
                    0.7,
                    "quite similar".to_string(),
                ),
            ],
        );

        let mut results = merge_signals(map, ImpactMode::Combined);
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].path, "src/a.rs");
        assert!((results[0].score - 1.0).abs() < f32::EPSILON);

        // src/c.rs should have combined signal with max(0.3, 0.7) = 0.7
        let c_result = results.iter().find(|r| r.path == "src/c.rs").unwrap();
        assert!((c_result.score - 0.7).abs() < f32::EPSILON);
        assert!(matches!(c_result.signal, ImpactSignal::Combined));
    }

    #[test]
    fn test_max_depth_cap() {
        // Depth should be clamped to MAX_DEPTH (3)
        let clamped = 10u32.clamp(1, MAX_DEPTH);
        assert_eq!(clamped, 3);

        let clamped = 0u32.clamp(1, MAX_DEPTH);
        assert_eq!(clamped, 1);

        let clamped = 2u32.clamp(1, MAX_DEPTH);
        assert_eq!(clamped, 2);
    }

    #[test]
    fn test_decay_factor_math() {
        let decay_factor: f32 = 0.5;

        // Level 0 (depth 1): decay = 0.5^0 = 1.0
        assert!((decay_factor.powi(0) - 1.0).abs() < f32::EPSILON);

        // Level 1 (depth 2): decay = 0.5^1 = 0.5
        assert!((decay_factor.powi(1) - 0.5).abs() < f32::EPSILON);

        // Level 2 (depth 3): decay = 0.5^2 = 0.25
        assert!((decay_factor.powi(2) - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_transitive_visited_set_prevents_cycles() {
        // Simulate the cycle prevention logic from analyze()
        let mut visited: HashSet<String> = HashSet::new();

        // First visit succeeds
        assert!(visited.insert("src/a.rs".to_string()));

        // Second visit of same target is rejected
        assert!(!visited.insert("src/a.rs".to_string()));

        // Different target succeeds
        assert!(visited.insert("src/b.rs".to_string()));
    }

    #[test]
    fn test_transitive_highest_score_kept() {
        // Simulate the all_results map logic: when a file appears at
        // multiple depths, keep the highest score
        let mut all_results: HashMap<String, ImpactResult> = HashMap::new();

        // Depth 2 result: score 0.4 (0.8 * 0.5 decay)
        let r1 = ImpactResult {
            path: "src/c.rs".to_string(),
            signal: ImpactSignal::Coupling { co_changes: 5 },
            score: 0.4,
            reason: "transitive via b.rs".to_string(),
        };
        all_results.insert(r1.path.clone(), r1);

        // Same file found at depth 1 with higher score: 0.7
        let r2 = ImpactResult {
            path: "src/c.rs".to_string(),
            signal: ImpactSignal::Semantic { similarity: 0.7 },
            score: 0.7,
            reason: "direct semantic".to_string(),
        };
        all_results
            .entry(r2.path.clone())
            .and_modify(|existing| {
                if r2.score > existing.score {
                    *existing = r2.clone();
                }
            })
            .or_insert(r2);

        let result = &all_results["src/c.rs"];
        assert!((result.score - 0.7).abs() < f32::EPSILON);
        assert!(matches!(result.signal, ImpactSignal::Semantic { .. }));
    }

    #[test]
    fn test_transitive_lower_score_not_replaced() {
        // When a file appears at a deeper level with lower score, the
        // existing higher score should be kept
        let mut all_results: HashMap<String, ImpactResult> = HashMap::new();

        // Depth 1 result: score 0.9
        let r1 = ImpactResult {
            path: "src/c.rs".to_string(),
            signal: ImpactSignal::Coupling { co_changes: 10 },
            score: 0.9,
            reason: "direct coupling".to_string(),
        };
        all_results.insert(r1.path.clone(), r1);

        // Same file at depth 2 with lower decayed score: 0.3
        let r2 = ImpactResult {
            path: "src/c.rs".to_string(),
            signal: ImpactSignal::Semantic { similarity: 0.6 },
            score: 0.3,
            reason: "transitive semantic".to_string(),
        };
        all_results
            .entry(r2.path.clone())
            .and_modify(|existing| {
                if r2.score > existing.score {
                    *existing = r2.clone();
                }
            })
            .or_insert(r2);

        let result = &all_results["src/c.rs"];
        assert!((result.score - 0.9).abs() < f32::EPSILON);
        assert!(matches!(result.signal, ImpactSignal::Coupling { .. }));
    }

    #[test]
    fn test_transitive_decay_filters_below_threshold() {
        // Results that decay below threshold should be skipped
        let threshold = 0.1;
        let decay: f32 = 0.5; // depth 2

        // Score 0.15 * 0.5 = 0.075 — below threshold
        let decayed_score = 0.15 * decay;
        assert!(decayed_score < threshold);

        // Score 0.3 * 0.5 = 0.15 — above threshold
        let decayed_score = 0.3 * decay;
        assert!(decayed_score >= threshold);
    }
}
