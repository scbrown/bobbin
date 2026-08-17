//! Shared embedding pipeline: bobbin's ONNX embedder, handed to quipu.
//!
//! Phase 2 of the quipu integration (bobbin-di7). Both systems need vectors of
//! the same text in the same space, and running two embedders in one process
//! would mean two ONNX sessions, two copies of the model in memory, and — the
//! part that actually breaks things — **two vector spaces whose scores are not
//! comparable**. Cosine similarity between vectors from different models is a
//! number, not a measurement.
//!
//! quipu already defines the seam (`quipu::embedding::EmbeddingProvider`) and
//! the setter (`Store::set_embedding_provider`). This is the bobbin side: a
//! thin adapter over `crate::index::Embedder`, so quipu auto-embeds entities on
//! write using the session bobbin already loaded.

use std::sync::Arc;

use crate::index::Embedder;

/// Adapter making bobbin's `Embedder` usable as quipu's embedding backend.
///
/// Holds the embedder by `Arc` rather than owning it because bobbin's indexer
/// and search paths need it too — the entire point is that there is one
/// session, not that quipu gets its own.
pub struct BobbinEmbeddingProvider {
    embedder: Arc<Embedder>,
}

impl BobbinEmbeddingProvider {
    pub fn new(embedder: Arc<Embedder>) -> Self {
        Self { embedder }
    }
}

impl quipu::embedding::EmbeddingProvider for BobbinEmbeddingProvider {
    fn embed_text(&self, text: &str) -> quipu::Result<Vec<f32>> {
        self.embedder
            .embed_sync(text)
            .map_err(|e| quipu::Error::InvalidValue(format!("bobbin embedder failed: {e}")))
    }

    /// Overridden rather than left to the trait's default loop: bobbin's
    /// embedder batches natively, and a per-item loop would pay the ONNX
    /// session overhead once per string.
    fn embed_batch(&self, texts: &[&str]) -> quipu::Result<Vec<Vec<f32>>> {
        self.embedder
            .embed_batch_sync(texts)
            .map_err(|e| quipu::Error::InvalidValue(format!("bobbin embedder failed: {e}")))
    }

    fn dimension(&self) -> usize {
        self.embedder.dimension()
    }
}

/// Attach bobbin's embedder to a quipu store.
///
/// Call this before any write that should auto-embed. A store without a
/// provider does not fail — it stores the facts and skips the vectors — so
/// forgetting this produces a graph that is silently unsearchable
/// semantically, which is why this is a named function with a doc comment
/// rather than two lines inlined at one call site.
pub fn attach_embedder(store: &mut quipu::Store, embedder: Arc<Embedder>) {
    store.set_embedding_provider(Arc::new(BobbinEmbeddingProvider::new(embedder)));
}

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod tests;
