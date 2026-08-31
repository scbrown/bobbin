//! Tests for `similar_to` edge persistence (sidecar of `similar_edges.rs`).
//!
//! Storage-level tests run on a tempdir Lance store, mirroring the existing
//! chunk-edge tests in `storage/lance_tests.rs`. No embedder and no model
//! files: candidates are built directly, since threshold gating is exercised
//! end-to-end in `similar_tests.rs` where the scan helpers live.

use super::*;
use crate::types::ChunkType;
use tempfile::tempdir;

fn chunk(id: &str, name: &str, file_path: &str) -> Chunk {
    Chunk {
        id: id.to_string(),
        file_path: file_path.to_string(),
        chunk_type: ChunkType::Function,
        name: Some(name.to_string()),
        start_line: 1,
        end_line: 10,
        content: format!("fn {}() {{ }}", name),
        language: "rust".to_string(),
        tags: String::new(),
    }
}

#[test]
fn edge_candidate_normalizes_direction() {
    let a = chunk("bbb", "later", "src/b.rs");
    let b = chunk("aaa", "earlier", "src/a.rs");

    // Passed in either order, the lexically smaller ID is the source.
    let c1 = edge_candidate(&a, "repo_a", &b, "repo_b", 0.95);
    let c2 = edge_candidate(&b, "repo_b", &a, "repo_a", 0.95);

    for c in [&c1, &c2] {
        assert_eq!(c.edge.source_chunk, "aaa");
        assert_eq!(c.edge.target_chunk, "bbb");
        assert_eq!(c.edge.source_name, "earlier");
        assert_eq!(c.edge.target_name, "later");
        assert_eq!(c.edge.edge_type, ChunkEdgeType::SimilarTo);
        // file_path and repo follow the (normalized) source chunk
        assert_eq!(c.edge.file_path, "src/a.rs");
        assert_eq!(c.repo, "repo_b");
    }
    assert_eq!(c1.similarity, 0.95);
}

#[tokio::test]
async fn persist_writes_edges_readable_by_chunk_id() {
    let dir = tempdir().unwrap();
    let mut store = VectorStore::open(&dir.path().join("vectors"))
        .await
        .unwrap();

    let candidates = vec![
        edge_candidate(
            &chunk("aaa", "fn_a", "src/a.rs"),
            "default",
            &chunk("bbb", "fn_b", "src/b.rs"),
            "default",
            0.97,
        ),
        edge_candidate(
            &chunk("aaa", "fn_a", "src/a.rs"),
            "default",
            &chunk("ccc", "fn_c", "src/c.rs"),
            "default",
            0.93,
        ),
    ];

    let written = persist_similar_edges(&mut store, &candidates, None)
        .await
        .unwrap();
    assert_eq!(written, 2);

    // Readable through the same by-id path chunk_neighbors uses.
    let edges = store.get_edges_for_chunk("aaa", None).await.unwrap();
    assert_eq!(edges.len(), 2);
    assert!(edges
        .iter()
        .all(|e| e.edge_type == ChunkEdgeType::SimilarTo));
    assert!(edges.iter().any(|e| e.target_chunk == "bbb"));
    assert!(edges.iter().any(|e| e.target_chunk == "ccc"));

    // And through the by-type path.
    let by_type = store
        .get_chunk_edges_by_type(ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert_eq!(by_type.len(), 2);
}

#[tokio::test]
async fn persist_rerun_is_idempotent() {
    let dir = tempdir().unwrap();
    let mut store = VectorStore::open(&dir.path().join("vectors"))
        .await
        .unwrap();

    let candidates = vec![edge_candidate(
        &chunk("aaa", "fn_a", "src/a.rs"),
        "default",
        &chunk("bbb", "fn_b", "src/b.rs"),
        "default",
        0.97,
    )];

    for _ in 0..3 {
        persist_similar_edges(&mut store, &candidates, None)
            .await
            .unwrap();
    }

    // Replace-not-accumulate: three runs converge to one row.
    let edges = store
        .get_chunk_edges_by_type(ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1, "re-runs must not accumulate duplicates");
}

#[tokio::test]
async fn persist_shrinks_when_pairs_disappear() {
    let dir = tempdir().unwrap();
    let mut store = VectorStore::open(&dir.path().join("vectors"))
        .await
        .unwrap();

    let two = vec![
        edge_candidate(
            &chunk("aaa", "fn_a", "src/a.rs"),
            "default",
            &chunk("bbb", "fn_b", "src/b.rs"),
            "default",
            0.97,
        ),
        edge_candidate(
            &chunk("ccc", "fn_c", "src/c.rs"),
            "default",
            &chunk("ddd", "fn_d", "src/d.rs"),
            "default",
            0.91,
        ),
    ];
    persist_similar_edges(&mut store, &two, None).await.unwrap();

    // A later scan (say, at a higher threshold) found only one pair —
    // the stale second edge must go away, not linger.
    persist_similar_edges(&mut store, &two[..1], None)
        .await
        .unwrap();

    let edges = store
        .get_chunk_edges_by_type(ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].source_chunk, "aaa");

    // And an empty scan clears the set entirely.
    persist_similar_edges(&mut store, &[], None).await.unwrap();
    let edges = store
        .get_chunk_edges_by_type(ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert!(edges.is_empty());
}

#[tokio::test]
async fn repo_scoped_persist_leaves_other_repos_alone() {
    let dir = tempdir().unwrap();
    let mut store = VectorStore::open(&dir.path().join("vectors"))
        .await
        .unwrap();

    let repo1 = vec![edge_candidate(
        &chunk("r1a", "fn_a", "src/a.rs"),
        "repo1",
        &chunk("r1b", "fn_b", "src/b.rs"),
        "repo1",
        0.96,
    )];
    let repo2 = vec![edge_candidate(
        &chunk("r2a", "fn_a", "src/a.rs"),
        "repo2",
        &chunk("r2b", "fn_b", "src/b.rs"),
        "repo2",
        0.94,
    )];

    persist_similar_edges(&mut store, &repo1, Some("repo1"))
        .await
        .unwrap();
    persist_similar_edges(&mut store, &repo2, Some("repo2"))
        .await
        .unwrap();

    // Re-persisting repo2 (even empty) must not touch repo1's edges.
    persist_similar_edges(&mut store, &[], Some("repo2"))
        .await
        .unwrap();

    let edges = store
        .get_chunk_edges_by_type(ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].source_chunk, "r1a");
}

#[tokio::test]
async fn persist_does_not_touch_structural_edges() {
    let dir = tempdir().unwrap();
    let mut store = VectorStore::open(&dir.path().join("vectors"))
        .await
        .unwrap();

    // A structural edge already in the table...
    let structural = ChunkEdge {
        source_chunk: "aaa".into(),
        target_chunk: "bbb".into(),
        source_name: "fn_a".into(),
        target_name: "fn_b".into(),
        edge_type: ChunkEdgeType::NextChunk,
        file_path: "src/a.rs".into(),
    };
    store
        .upsert_chunk_edges(&[structural], "default")
        .await
        .unwrap();

    // ...survives an unscoped similar_to replace.
    persist_similar_edges(&mut store, &[], None).await.unwrap();

    let edges = store.get_edges_for_chunk("aaa", None).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].edge_type, ChunkEdgeType::NextChunk);
}
