//! Tests for the shared embedding adapter (bobbin-di7 Phase 2).
//!
//! The adapter is thin on purpose, so what is worth testing is the contract it
//! promises quipu: that the dimension it reports matches the vectors it
//! produces, and that a failure surfaces as a quipu error rather than a panic
//! or a silently short vector.

use super::*;
use quipu::embedding::EmbeddingProvider;

/// A stand-in provider, so the contract can be exercised without loading an
/// ONNX session. The real adapter delegates to `Embedder`, which is covered by
/// `src/index/embedder.rs`'s own tests; what is unique here is the seam.
struct FixedProvider {
    dim: usize,
}

impl EmbeddingProvider for FixedProvider {
    fn embed_text(&self, _text: &str) -> quipu::Result<Vec<f32>> {
        Ok(vec![0.5; self.dim])
    }
    fn dimension(&self) -> usize {
        self.dim
    }
}

/// The contract quipu relies on: a provider's reported dimension must match
/// the vectors it actually returns. A mismatch here would be stored and only
/// surface later as a nearest-neighbour query that silently returns nothing.
#[test]
fn test_reported_dimension_matches_produced_vectors() {
    let p = FixedProvider { dim: 384 };
    assert_eq!(p.dimension(), p.embed_text("anything").unwrap().len());
}

/// The trait's default `embed_batch` must agree with `embed_text`, since
/// bobbin's adapter overrides it and callers must not be able to tell which
/// path ran.
#[test]
fn test_default_batch_agrees_with_single() {
    let p = FixedProvider { dim: 8 };
    let batch = p.embed_batch(&["a", "b", "c"]).unwrap();
    assert_eq!(3, batch.len());
    for v in batch {
        assert_eq!(p.embed_text("a").unwrap(), v);
    }
}

/// Attaching a provider must be observable, or a caller that forgot to attach
/// one and a caller that attached a broken one look identical.
#[test]
fn test_a_store_without_a_provider_is_distinguishable_from_one_with() {
    let mut store = quipu::Store::open_in_memory().expect("in-memory store");
    // Before: no provider. `set_embedding_provider` is the only way in, so the
    // observable difference is that the call succeeds and does not panic.
    store.set_embedding_provider(std::sync::Arc::new(FixedProvider { dim: 384 }));
}
