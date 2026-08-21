//! Tests for `compute_session_id`'s top-10 selection (bobbin-zhx).
//!
//! In their own file because `src/cli/hook.rs` is the largest file in the tree
//! and is allowlisted out of the file-size gate; growing it further by 100
//! lines of tests is exactly the drift the allowlist is supposed to bound.

use super::*;
use crate::search::context::{
    BudgetInfo, ContextBundle, ContextChunk, ContextFile, ContextSummary, FileRelevance,
};
use crate::types::{ChunkType, FileCategory};

/// Build a bundle of one-chunk files from `(path, score)` pairs.
fn bundle_of(files: &[(&str, f32)]) -> ContextBundle {
    ContextBundle {
        query: "q".to_string(),
        files: files
            .iter()
            .map(|(path, score)| ContextFile {
                path: (*path).to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: FileCategory::Source,
                score: *score,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    id: String::new(),
                    name: None,
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 2,
                    score: *score,
                    match_type: None,
                    content: None,
                }],
            })
            .collect(),
        budget: BudgetInfo {
            max_lines: 150,
            used_lines: 0,
            pinned_lines: 0,
        },
        summary: ContextSummary {
            structural_additions: 0,
            total_files: files.len(),
            total_chunks: files.len(),
            direct_hits: files.len(),
            coupled_additions: 0,
            bridged_additions: 0,
            source_files: files.len(),
            doc_files: 0,
            top_semantic_score: 0.0,
            pinned_chunks: 0,
            knowledge_additions: 0,
        },
    }
}

/// The defect as filed: with more than ten candidates, selection was by path
/// lexicography, so a high-scoring `src/z…` lost its slot to a low-scoring
/// `src/a…`. Two bundles that share their ten *best* chunks and differ only in
/// irrelevant low scorers must now fingerprint identically.
#[test]
fn test_selection_is_by_score_not_by_path_lexicography() {
    // Ten strong chunks, all late in the alphabet.
    let mut strong: Vec<(&str, f32)> = vec![
        ("src/z01.rs", 0.90),
        ("src/z02.rs", 0.91),
        ("src/z03.rs", 0.92),
        ("src/z04.rs", 0.93),
        ("src/z05.rs", 0.94),
        ("src/z06.rs", 0.95),
        ("src/z07.rs", 0.96),
        ("src/z08.rs", 0.97),
        ("src/z09.rs", 0.98),
        ("src/z10.rs", 0.99),
    ];
    let only_strong = compute_session_id(&bundle_of(&strong), 0.0);

    // Add weak chunks that sort *before* every strong one. Pre-fix these
    // displaced the entire top ten and changed the fingerprint.
    strong.extend_from_slice(&[
        ("src/a01.rs", 0.10),
        ("src/a02.rs", 0.11),
        ("src/a03.rs", 0.12),
    ]);
    let with_weak = compute_session_id(&bundle_of(&strong), 0.0);

    assert_eq!(
        only_strong, with_weak,
        "adding low-scoring chunks that sort early changed the fingerprint — \
         selection is still lexicographic",
    );
}

/// The subtle half of the fix: the final `keys.sort()` must come *after* the
/// truncate, so the hash is over the selected set and not over score order.
/// Two bundles holding the same ten chunks at different relative scores must
/// fingerprint the same.
#[test]
fn test_fingerprint_is_stable_when_scores_reorder_without_changing_the_set() {
    let ascending = bundle_of(&[("src/a.rs", 0.10), ("src/b.rs", 0.20), ("src/c.rs", 0.30)]);
    let descending = bundle_of(&[("src/a.rs", 0.30), ("src/b.rs", 0.20), ("src/c.rs", 0.10)]);

    assert_eq!(
        compute_session_id(&ascending, 0.0),
        compute_session_id(&descending, 0.0),
        "the fingerprint moved when only the score ORDER changed; the final \
         sort must happen after the truncate",
    );
}

/// A genuinely different set must still produce a different fingerprint —
/// the guard that keeps the test above from passing vacuously.
#[test]
fn test_a_different_selected_set_still_changes_the_fingerprint() {
    let a = bundle_of(&[("src/a.rs", 0.5), ("src/b.rs", 0.5)]);
    let b = bundle_of(&[("src/a.rs", 0.5), ("src/c.rs", 0.5)]);
    assert_ne!(compute_session_id(&a, 0.0), compute_session_id(&b, 0.0));
}

/// Ties must not fall through to bundle iteration order. `sort_by` is stable,
/// so without an explicit key tie-break the fingerprint would depend on the
/// order the assembly pipeline happened to emit files in.
#[test]
fn test_ties_break_deterministically_by_key_not_by_bundle_order() {
    let forward = bundle_of(&[
        ("src/a.rs", 0.5),
        ("src/b.rs", 0.5),
        ("src/c.rs", 0.5),
        ("src/d.rs", 0.5),
        ("src/e.rs", 0.5),
        ("src/f.rs", 0.5),
        ("src/g.rs", 0.5),
        ("src/h.rs", 0.5),
        ("src/i.rs", 0.5),
        ("src/j.rs", 0.5),
        ("src/k.rs", 0.5),
    ]);
    let reversed_files = [
        ("src/k.rs", 0.5),
        ("src/j.rs", 0.5),
        ("src/i.rs", 0.5),
        ("src/h.rs", 0.5),
        ("src/g.rs", 0.5),
        ("src/f.rs", 0.5),
        ("src/e.rs", 0.5),
        ("src/d.rs", 0.5),
        ("src/c.rs", 0.5),
        ("src/b.rs", 0.5),
        ("src/a.rs", 0.5),
    ];

    assert_eq!(
        compute_session_id(&forward, 0.0),
        compute_session_id(&bundle_of(&reversed_files), 0.0),
        "an all-ties bundle fingerprinted differently depending on file order",
    );
}

/// The threshold filter still applies, and still applies *before* selection.
#[test]
fn test_threshold_filters_before_selection() {
    let b = bundle_of(&[("src/a.rs", 0.9), ("src/b.rs", 0.1)]);
    let above_only = bundle_of(&[("src/a.rs", 0.9)]);
    assert_eq!(
        compute_session_id(&b, 0.5),
        compute_session_id(&above_only, 0.5),
    );
}

/// Selection caps at ten. An eleventh chunk below the cut must not move it.
#[test]
fn test_selection_caps_at_ten() {
    let ten: Vec<(&str, f32)> = vec![
        ("src/a.rs", 0.99),
        ("src/b.rs", 0.98),
        ("src/c.rs", 0.97),
        ("src/d.rs", 0.96),
        ("src/e.rs", 0.95),
        ("src/f.rs", 0.94),
        ("src/g.rs", 0.93),
        ("src/h.rs", 0.92),
        ("src/i.rs", 0.91),
        ("src/j.rs", 0.90),
    ];
    let mut eleven = ten.clone();
    eleven.push(("src/k.rs", 0.01));

    assert_eq!(
        compute_session_id(&bundle_of(&ten), 0.0),
        compute_session_id(&bundle_of(&eleven), 0.0),
        "an 11th chunk below the cut changed the fingerprint",
    );
}

/// An empty bundle must still hash, not panic.
#[test]
fn test_empty_bundle_hashes() {
    let id = compute_session_id(&bundle_of(&[]), 0.0);
    assert_eq!(id.len(), 16);
}
