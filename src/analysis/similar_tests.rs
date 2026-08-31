//! Tests for similarity search and the near-duplicate scan
//! (sidecar of `similar.rs`).

use super::*;
use crate::storage::VectorStore;
use crate::types::ChunkType;
use tempfile::tempdir;

fn sample_chunk(id: &str, name: &str, file_path: &str) -> Chunk {
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

fn make_embedding(seed: f32) -> Vec<f32> {
    let mut emb: Vec<f32> = (0..384).map(|i| ((i as f32) + seed) / 384.0).collect();
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    emb.iter_mut().for_each(|x| *x /= norm);
    emb
}

fn no_contexts(n: usize) -> Vec<Option<String>> {
    vec![None; n]
}

#[test]
fn test_parse_chunk_ref_valid() {
    let (file, name) = parse_chunk_ref("src/main.rs:process_data").unwrap();
    assert_eq!(file, "src/main.rs");
    assert_eq!(name, "process_data");
}

#[test]
fn test_parse_chunk_ref_nested_path() {
    let (file, name) = parse_chunk_ref("src/handlers/auth.rs:verify_token").unwrap();
    assert_eq!(file, "src/handlers/auth.rs");
    assert_eq!(name, "verify_token");
}

#[test]
fn test_parse_chunk_ref_no_colon() {
    assert!(parse_chunk_ref("src/main.rs").is_err());
}

#[test]
fn test_parse_chunk_ref_empty_name() {
    assert!(parse_chunk_ref("src/main.rs:").is_err());
}

#[test]
fn test_parse_chunk_ref_empty_file() {
    assert!(parse_chunk_ref(":func").is_err());
}

#[test]
fn test_build_explanation_with_name() {
    let result = SearchResult {
        chunk: Chunk {
            id: "id1".to_string(),
            file_path: "src/auth.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("verify_token".to_string()),
            start_line: 10,
            end_line: 20,
            content: "fn verify_token() {}".to_string(),
            language: "rust".to_string(),
            tags: String::new(),
        },
        score: 0.9,
        match_type: Some(crate::types::MatchType::Semantic),
        indexed_at: None,
        repo: None,
    };

    let explanation = build_explanation(&result);
    assert_eq!(explanation, "function 'verify_token' in src/auth.rs");
}

#[test]
fn test_build_explanation_without_name() {
    let result = SearchResult {
        chunk: Chunk {
            id: "id1".to_string(),
            file_path: "src/auth.rs".to_string(),
            chunk_type: ChunkType::Section,
            name: None,
            start_line: 10,
            end_line: 20,
            content: "some section".to_string(),
            language: "rust".to_string(),
            tags: String::new(),
        },
        score: 0.9,
        match_type: Some(crate::types::MatchType::Semantic),
        indexed_at: None,
        repo: None,
    };

    let explanation = build_explanation(&result);
    assert_eq!(explanation, "section in src/auth.rs (lines 10-20)");
}

/// Helper: directly test find_similar logic using raw VectorStore
/// (bypasses embedder since ChunkRef path uses stored embeddings)
#[tokio::test]
async fn test_find_similar_via_chunk_ref() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Insert three chunks with different embeddings
    let emb_target = make_embedding(0.0);
    let emb_similar = make_embedding(1.0); // Very close to target
    let emb_different = make_embedding(500.0); // Very different

    let chunks = vec![
        sample_chunk("target", "process_data", "src/main.rs"),
        sample_chunk("similar", "process_items", "src/utils.rs"),
        sample_chunk("different", "render_html", "src/views.rs"),
    ];
    let embeddings = vec![emb_target.clone(), emb_similar, emb_different];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(3),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Manually resolve chunk ref (what SimilarityAnalyzer.resolve_chunk_ref does)
    let file_chunks = store
        .get_chunks_for_file("src/main.rs", None)
        .await
        .unwrap();
    let target_chunk = file_chunks
        .iter()
        .find(|c| c.name.as_deref() == Some("process_data"))
        .unwrap();

    // Get stored embedding
    let stored_emb = store
        .get_chunk_embedding(&target_chunk.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_emb.len(), 384);

    // Search with the stored embedding
    let results = store.search(&stored_emb, 10, None).await.unwrap();

    // Should find all 3 chunks (including self)
    assert_eq!(results.len(), 3);

    // Filter: exclude self, apply threshold
    let filtered: Vec<_> = results
        .into_iter()
        .filter(|r| r.chunk.id != target_chunk.id)
        .filter(|r| r.score >= 0.0)
        .collect();

    // Target should not be in filtered results
    assert!(!filtered.iter().any(|r| r.chunk.id == "target"));

    // Results should be ordered by score descending (VectorStore returns them this way)
    for w in filtered.windows(2) {
        assert!(w[0].score >= w[1].score);
    }
}

#[tokio::test]
async fn test_threshold_filtering_logic() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let emb_target = make_embedding(0.0);
    let emb_close = make_embedding(0.5);
    let emb_far = make_embedding(1000.0);

    let chunks = vec![
        sample_chunk("target", "func_a", "src/a.rs"),
        sample_chunk("close", "func_b", "src/b.rs"),
        sample_chunk("far", "func_c", "src/c.rs"),
    ];

    store
        .insert(
            &chunks,
            &[emb_target.clone(), emb_close, emb_far],
            &no_contexts(3),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let results = store.search(&emb_target, 10, None).await.unwrap();

    // With a very high threshold, only very similar results should pass
    let high_threshold: Vec<_> = results
        .iter()
        .filter(|r| r.chunk.id != "target")
        .filter(|r| r.score >= 0.99)
        .collect();

    // All results should meet the threshold
    for r in &high_threshold {
        assert!(r.score >= 0.99);
    }
}

#[tokio::test]
async fn test_limit_enforcement() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Insert several chunks with the same embedding
    let emb = make_embedding(0.0);
    let chunks: Vec<Chunk> = (0..5)
        .map(|i| {
            sample_chunk(
                &format!("c{}", i),
                &format!("func_{}", i),
                &format!("src/f{}.rs", i),
            )
        })
        .collect();
    let embeddings: Vec<Vec<f32>> = (0..5).map(|_| emb.clone()).collect();

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(5),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Request limit of 3 from vector search (+1 for self-exclusion)
    let results = store.search(&emb, 3, None).await.unwrap();

    // Exclude self and enforce limit of 2
    let limited: Vec<_> = results
        .into_iter()
        .filter(|r| r.chunk.id != "c0")
        .take(2)
        .collect();

    assert!(limited.len() <= 2);
}

#[tokio::test]
async fn test_resolve_chunk_ref_finds_chunk() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let chunks = vec![
        sample_chunk("c1", "authenticate", "src/auth.rs"),
        sample_chunk("c2", "verify_token", "src/auth.rs"),
    ];
    let embeddings = vec![make_embedding(0.0), make_embedding(100.0)];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(2),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Manually test the chunk resolution logic
    let (file_path, chunk_name) = parse_chunk_ref("src/auth.rs:verify_token").unwrap();
    let file_chunks = store.get_chunks_for_file(file_path, None).await.unwrap();
    let chunk = file_chunks
        .into_iter()
        .find(|c| c.name.as_deref() == Some(chunk_name))
        .unwrap();

    assert_eq!(chunk.id, "c2");
    assert_eq!(chunk.name, Some("verify_token".to_string()));

    let embedding = store.get_chunk_embedding(&chunk.id).await.unwrap().unwrap();
    assert_eq!(embedding.len(), 384);
}

#[tokio::test]
async fn test_resolve_chunk_ref_missing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let store = VectorStore::open(&path).await.unwrap();

    // Empty store -- no chunks for any file
    let chunks = store
        .get_chunks_for_file("nonexistent.rs", None)
        .await
        .unwrap();
    assert!(chunks.is_empty());
}

#[tokio::test]
async fn test_resolve_chunk_ref_missing_name() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let chunks = vec![sample_chunk("c1", "authenticate", "src/auth.rs")];
    let embeddings = vec![make_embedding(0.0)];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(1),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let file_chunks = store
        .get_chunks_for_file("src/auth.rs", None)
        .await
        .unwrap();
    let found = file_chunks
        .iter()
        .find(|c| c.name.as_deref() == Some("nonexistent"));
    assert!(found.is_none());
}

#[tokio::test]
async fn test_self_exclusion() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let emb = make_embedding(0.0);
    let chunks = vec![
        sample_chunk("c1", "func_a", "src/a.rs"),
        sample_chunk("c2", "func_b", "src/b.rs"),
    ];

    store
        .insert(
            &chunks,
            &[emb.clone(), emb.clone()],
            &no_contexts(2),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Get the embedding for c1
    let stored = store.get_chunk_embedding("c1").await.unwrap().unwrap();

    // Search should return both
    let results = store.search(&stored, 10, None).await.unwrap();
    assert_eq!(results.len(), 2);

    // After filtering out self
    let without_self: Vec<_> = results.into_iter().filter(|r| r.chunk.id != "c1").collect();
    assert_eq!(without_self.len(), 1);
    assert_eq!(without_self[0].chunk.id, "c2");
}

#[tokio::test]
async fn test_results_ordered_by_similarity() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let emb_target = make_embedding(0.0);
    let emb_close = make_embedding(1.0);
    let emb_medium = make_embedding(50.0);
    let emb_far = make_embedding(500.0);

    let chunks = vec![
        sample_chunk("target", "target_fn", "src/target.rs"),
        sample_chunk("close", "close_fn", "src/close.rs"),
        sample_chunk("medium", "medium_fn", "src/medium.rs"),
        sample_chunk("far", "far_fn", "src/far.rs"),
    ];

    store
        .insert(
            &chunks,
            &[emb_target.clone(), emb_close, emb_medium, emb_far],
            &no_contexts(4),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let results = store.search(&emb_target, 10, None).await.unwrap();
    let filtered: Vec<_> = results
        .into_iter()
        .filter(|r| r.chunk.id != "target")
        .collect();

    // Results should be ordered by score descending
    for w in filtered.windows(2) {
        assert!(
            w[0].score >= w[1].score,
            "Results not in descending order: {} >= {} failed",
            w[0].score,
            w[1].score
        );
    }
}

// ── Union-find tests ──────────────────────────────────────────────

#[test]
fn test_union_find_basic() {
    let mut uf = UnionFind::new(5);
    uf.union(0, 1);
    uf.union(2, 3);
    assert_eq!(uf.find(0), uf.find(1));
    assert_eq!(uf.find(2), uf.find(3));
    assert_ne!(uf.find(0), uf.find(2));

    // Now merge the two groups
    uf.union(1, 3);
    assert_eq!(uf.find(0), uf.find(3));
    // 4 is still isolated
    assert_ne!(uf.find(0), uf.find(4));
}

#[test]
fn test_union_find_single_element() {
    let mut uf = UnionFind::new(1);
    assert_eq!(uf.find(0), 0);
}

// ── Scan duplicates tests ─────────────────────────────────────────

#[tokio::test]
async fn test_scan_finds_duplicate_clusters() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Create two groups of near-identical chunks
    let emb_group1 = make_embedding(0.0);
    let emb_group1b = make_embedding(0.5); // very close to group1
    let emb_group2 = make_embedding(500.0);
    let emb_group2b = make_embedding(500.5); // very close to group2

    let chunks = vec![
        sample_chunk("g1a", "func_a1", "src/a.rs"),
        sample_chunk("g1b", "func_a2", "src/b.rs"),
        sample_chunk("g2a", "func_b1", "src/c.rs"),
        sample_chunk("g2b", "func_b2", "src/d.rs"),
    ];
    let embeddings = vec![emb_group1, emb_group1b, emb_group2, emb_group2b];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(4),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Scan with a moderate threshold
    let (clusters, _) = scan_duplicates_impl(&store, 0.80, 10, None, true)
        .await
        .unwrap();

    // Should find at least one cluster
    assert!(
        !clusters.is_empty(),
        "Expected at least one duplicate cluster"
    );

    // Each cluster should have at least 2 members (rep + members)
    for cluster in &clusters {
        assert!(
            !cluster.members.is_empty(),
            "Cluster should have at least one member besides representative"
        );
    }
}

#[tokio::test]
async fn test_scan_no_duplicates_high_threshold() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Very different embeddings - no duplicates at high threshold
    let chunks = vec![
        sample_chunk("c1", "func1", "src/a.rs"),
        sample_chunk("c2", "func2", "src/b.rs"),
    ];
    let embeddings = vec![make_embedding(0.0), make_embedding(1000.0)];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(2),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let (clusters, _) = scan_duplicates_impl(&store, 0.999, 10, None, true)
        .await
        .unwrap();

    assert!(
        clusters.is_empty(),
        "Expected no clusters with very high threshold"
    );
}

#[tokio::test]
async fn test_scan_empty_store() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let store = VectorStore::open(&path).await.unwrap();

    let (clusters, _) = scan_duplicates_impl(&store, 0.90, 10, None, true)
        .await
        .unwrap();

    assert!(clusters.is_empty());
}

#[tokio::test]
async fn test_scan_deduplicates_pairs() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Two identical embeddings
    let emb = make_embedding(0.0);
    let chunks = vec![
        sample_chunk("a", "func_a", "src/a.rs"),
        sample_chunk("b", "func_b", "src/b.rs"),
    ];

    store
        .insert(
            &chunks,
            &[emb.clone(), emb.clone()],
            &no_contexts(2),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let (clusters, _) = scan_duplicates_impl(&store, 0.50, 10, None, true)
        .await
        .unwrap();

    // Should produce exactly one cluster with 2 chunks
    assert_eq!(clusters.len(), 1, "Expected exactly one cluster");
    assert_eq!(
        clusters[0].members.len(),
        1,
        "Cluster should have 1 member + representative"
    );
}

#[tokio::test]
async fn test_scan_max_clusters_limit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Create 3 distinct pairs (6 chunks), each pair identical
    let mut chunks = Vec::new();
    let mut embeddings = Vec::new();
    for pair in 0..3 {
        let emb = make_embedding(pair as f32 * 500.0);
        chunks.push(sample_chunk(
            &format!("p{}a", pair),
            &format!("func_{}a", pair),
            &format!("src/p{}a.rs", pair),
        ));
        chunks.push(sample_chunk(
            &format!("p{}b", pair),
            &format!("func_{}b", pair),
            &format!("src/p{}b.rs", pair),
        ));
        embeddings.push(emb.clone());
        embeddings.push(emb);
    }

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(6),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    // Limit to 2 clusters
    let (clusters, _) = scan_duplicates_impl(&store, 0.50, 2, None, true)
        .await
        .unwrap();

    assert!(clusters.len() <= 2, "Should respect max_clusters limit");
}

#[tokio::test]
async fn test_scan_cross_repo_filtering() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Two identical chunks in different repos
    let emb = make_embedding(0.0);
    let c1 = sample_chunk("r1c1", "func_a", "src/a.rs");
    let c2 = sample_chunk("r2c1", "func_a", "src/a.rs");

    store
        .insert(
            &[c1],
            &[emb.clone()],
            &no_contexts(1),
            "repo1",
            "abc",
            "100",
        )
        .await
        .unwrap();
    store
        .insert(
            &[c2],
            &[emb.clone()],
            &no_contexts(1),
            "repo2",
            "def",
            "100",
        )
        .await
        .unwrap();

    // cross_repo=false should NOT find duplicates across repos
    let (clusters_same_repo, _) = scan_duplicates_impl(&store, 0.50, 10, None, false)
        .await
        .unwrap();
    assert!(
        clusters_same_repo.is_empty(),
        "cross_repo=false should not find cross-repo duplicates"
    );

    // cross_repo=true should find the cross-repo pair
    let (clusters_cross, _) = scan_duplicates_impl(&store, 0.50, 10, None, true)
        .await
        .unwrap();
    assert!(
        !clusters_cross.is_empty(),
        "cross_repo=true should find cross-repo duplicates"
    );
}

/// Make an embedding that is orthogonal to the standard make_embedding vectors.
/// Uses alternating sign pattern to create a distinct direction.
fn make_orthogonal_embedding(seed: f32) -> Vec<f32> {
    let mut emb: Vec<f32> = (0..384)
        .map(|i| {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            sign * ((i as f32) + seed) / 384.0
        })
        .collect();
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    emb.iter_mut().for_each(|x| *x /= norm);
    emb
}

#[tokio::test]
async fn test_scan_clusters_sorted_by_size() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // Create one cluster of 3 (identical embeddings) and one cluster of 2
    // Use orthogonal embeddings so the groups are clearly separated
    let emb_big = make_embedding(0.0);
    let emb_small = make_orthogonal_embedding(0.0);

    let chunks = vec![
        sample_chunk("big1", "fn_a", "src/a.rs"),
        sample_chunk("big2", "fn_b", "src/b.rs"),
        sample_chunk("big3", "fn_c", "src/c.rs"),
        sample_chunk("small1", "fn_d", "src/d.rs"),
        sample_chunk("small2", "fn_e", "src/e.rs"),
    ];
    let embeddings = vec![
        emb_big.clone(),
        emb_big.clone(),
        emb_big,
        emb_small.clone(),
        emb_small,
    ];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(5),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let (clusters, _) = scan_duplicates_impl(&store, 0.50, 10, None, true)
        .await
        .unwrap();

    assert!(
        clusters.len() >= 2,
        "Expected at least 2 clusters, got {}",
        clusters.len()
    );

    // First cluster should be the bigger one (3 members total)
    let first_size = clusters[0].members.len() + 1;
    let second_size = clusters[1].members.len() + 1;
    assert!(
        first_size >= second_size,
        "Clusters should be sorted by size (largest first): {} vs {}",
        first_size,
        second_size
    );
}

// ── Edge-candidate tests (bobbin-363) ─────────────────────────────

#[tokio::test]
async fn scan_emits_threshold_gated_edge_candidates() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    // One near-identical pair, one unrelated chunk.
    let chunks = vec![
        sample_chunk("dup_a", "fn_a", "src/a.rs"),
        sample_chunk("dup_b", "fn_b", "src/b.rs"),
        sample_chunk("lonely", "fn_c", "src/c.rs"),
    ];
    let embeddings = vec![
        make_embedding(0.0),
        make_embedding(0.1),
        make_orthogonal_embedding(0.0),
    ];

    store
        .insert(
            &chunks,
            &embeddings,
            &no_contexts(3),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    let (_, candidates) = scan_duplicates_impl(&store, 0.90, 10, None, true)
        .await
        .unwrap();

    // Only the duplicate pair clears the gate, normalized a->b.
    assert_eq!(candidates.len(), 1);
    let c = &candidates[0];
    assert_eq!(c.edge.source_chunk, "dup_a");
    assert_eq!(c.edge.target_chunk, "dup_b");
    assert!(c.similarity >= 0.90);

    // A threshold no pair clears yields no candidates.
    let (_, none) = scan_duplicates_impl(&store, 1.01, 10, None, true)
        .await
        .unwrap();
    assert!(none.is_empty());
}

#[tokio::test]
async fn scan_persist_roundtrip_is_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vectors");
    let mut store = VectorStore::open(&path).await.unwrap();

    let emb = make_embedding(0.0);
    let chunks = vec![
        sample_chunk("aaa", "fn_a", "src/a.rs"),
        sample_chunk("bbb", "fn_b", "src/b.rs"),
    ];
    store
        .insert(
            &chunks,
            &[emb.clone(), emb],
            &no_contexts(2),
            "default",
            "abc",
            "100",
        )
        .await
        .unwrap();

    for _ in 0..2 {
        let (_, candidates) = scan_duplicates_impl(&store, 0.90, 10, None, true)
            .await
            .unwrap();
        crate::analysis::similar_edges::persist_similar_edges(&mut store, &candidates, None)
            .await
            .unwrap();
    }

    let edges = store
        .get_chunk_edges_by_type(crate::types::ChunkEdgeType::SimilarTo)
        .await
        .unwrap();
    assert_eq!(edges.len(), 1, "scan+persist re-run must converge");
    assert_eq!(edges[0].source_chunk, "aaa");
    assert_eq!(edges[0].target_chunk, "bbb");
}
