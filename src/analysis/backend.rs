//! The structural-capability backend contract (swappable engine seam).
//!
//! Bobbin's structural operations — find_refs, list_symbols, impact — are
//! implemented today over the vector-store INDEX: lexical name matching on
//! indexed chunks. That is honest about its limits (`RefAnalyzer`'s own docs:
//! false positives in comments/strings, no rename tracking) and it is coupled
//! to index freshness. A structural ENGINE (tree-sitter parse graphs, live
//! per-tenant structure, position-based precision) can answer the same
//! questions strictly better.
//!
//! This trait is the seam between the two: bobbin remains the semantic-search
//! and orchestration front, and the STRUCTURAL ops route through a
//! [`StructuralBackend`] — today the index-backed implementation, tomorrow an
//! engine backend at op-by-op granularity. Migrating an op across the seam
//! must be a STRICT UPGRADE: same signature, same-or-better answers. The
//! contract deliberately reuses bobbin's existing result types
//! ([`SymbolRefs`], [`FileSymbols`], [`ImpactResult`]) so the seam is
//! swappable WITHOUT a result-shape migration — an engine backend maps its
//! richer answers INTO these shapes (and may extend them additively later).
//!
//! Engine-backend ground rules (for the implementation behind this seam):
//! - Transport: the engine's resident daemon HTTP surface where present, CLI
//!   invocation as fallback. Never a second in-process parser stack inside
//!   bobbin — the engine owns structure, bobbin owns search.
//! - Fallback: an engine error is surfaced as an error, NOT silently answered
//!   from the index — a silent downgrade would make precision
//!   environment-dependent, which is the "same signal, different referent"
//!   failure class this workspace keeps paying for. Callers choose fallback
//!   explicitly if they want it.
//! - Capability probing: whether the engine serves a given op is asked at the
//!   seam (per-op), not assumed from its presence on the host.

use anyhow::Result;
use async_trait::async_trait;

use super::impact::{ImpactAnalyzer, ImpactConfig, ImpactResult};
use super::refs::{FileSymbols, RefAnalyzer, SymbolRefs};
use crate::index::Embedder;
use crate::storage::{MetadataStore, VectorStore};

/// The ops a [`StructuralBackend`] may serve, for per-op capability probing.
///
/// Whether a backend serves an op is a property of the CONSTRUCTED backend
/// (which stores/engine endpoints it was given), not of the type — probe with
/// [`StructuralBackend::supports`] before calling; an unsupported op errors
/// loudly rather than downgrading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralOp {
    FindRefs,
    ListSymbols,
    Impact,
}

/// The structural operations bobbin exposes, as a swappable capability.
///
/// Op-by-op notes on what "strict upgrade" means for an engine backend:
/// - `find_refs`: index backend is lexical (name-match over chunks); an engine
///   backend resolves references structurally (no comment/string false
///   positives, rename-aware at the precision tier).
/// - `list_symbols`: index backend reads what the indexer extracted at last
///   index time; an engine backend parses the file as it is NOW.
/// - `impact`: index backend approximates blast radius from co-occurrence; an
///   engine backend walks a real call graph ("what breaks", transitively).
#[async_trait]
pub trait StructuralBackend: Send {
    /// Human-readable backend identity, for surfacing WHICH engine answered —
    /// precision differs across backends, and an answer that does not say
    /// where it came from cannot be trusted at the right tier.
    fn name(&self) -> &'static str;

    /// Whether this backend, as constructed, serves the given op. Callers
    /// probe here instead of assuming from the backend's presence; calling an
    /// unsupported op is an error, never a silent downgrade.
    fn supports(&self, op: StructuralOp) -> bool;

    /// Find a symbol's definition(s) and usages.
    async fn find_refs(
        &mut self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SymbolRefs>;

    /// List the symbols defined in one file.
    async fn list_symbols(&mut self, file_path: &str, repo: Option<&str>) -> Result<FileSymbols>;

    /// Blast radius of changing `target`.
    async fn impact(
        &mut self,
        target: &str,
        config: &ImpactConfig,
        depth: u32,
        repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>>;
}

/// The index-backed implementation: bobbin's existing analyzers, unchanged in
/// behavior, now reachable through the seam. This is the reference
/// implementation an engine backend must meet or beat per-op.
///
/// Holds the store handles directly and constructs the analyzers per-op, so
/// one backend can serve ops with different store needs (`find_refs` /
/// `list_symbols` need the vector store; `impact` additionally needs the
/// metadata store and embedder). Impact capability is what it was CONSTRUCTED
/// with — [`Self::new`] serves refs/symbols only, [`Self::with_impact`] all
/// three — and is reported honestly through `supports`.
pub struct IndexBackend<'a> {
    vector_store: &'a mut VectorStore,
    impact_deps: Option<ImpactDeps<'a>>,
}

/// The extra stores `impact` needs beyond the vector store. Mutable borrows
/// for the same `Send`-without-`Sync` reason as [`ImpactAnalyzer`].
struct ImpactDeps<'a> {
    metadata_store: &'a mut MetadataStore,
    embedder: &'a mut Embedder,
}

impl<'a> IndexBackend<'a> {
    /// A backend serving `find_refs` and `list_symbols`; `impact` is
    /// unsupported (probe `supports`, calls error loudly).
    pub fn new(vector_store: &'a mut VectorStore) -> Self {
        Self {
            vector_store,
            impact_deps: None,
        }
    }

    /// A backend serving all ops, including `impact`.
    pub fn with_impact(
        vector_store: &'a mut VectorStore,
        metadata_store: &'a mut MetadataStore,
        embedder: &'a mut Embedder,
    ) -> Self {
        Self {
            vector_store,
            impact_deps: Some(ImpactDeps {
                metadata_store,
                embedder,
            }),
        }
    }
}

#[async_trait]
impl<'a> StructuralBackend for IndexBackend<'a> {
    fn name(&self) -> &'static str {
        "index"
    }

    fn supports(&self, op: StructuralOp) -> bool {
        match op {
            StructuralOp::FindRefs | StructuralOp::ListSymbols => true,
            StructuralOp::Impact => self.impact_deps.is_some(),
        }
    }

    async fn find_refs(
        &mut self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SymbolRefs> {
        RefAnalyzer::new(self.vector_store)
            .find_refs(symbol_name, symbol_type, limit, repo)
            .await
    }

    async fn list_symbols(&mut self, file_path: &str, repo: Option<&str>) -> Result<FileSymbols> {
        RefAnalyzer::new(self.vector_store)
            .list_symbols(file_path, repo)
            .await
    }

    async fn impact(
        &mut self,
        target: &str,
        config: &ImpactConfig,
        depth: u32,
        repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>> {
        let Some(deps) = self.impact_deps.as_mut() else {
            // Loud, not a downgrade: the caller constructed this backend
            // without the impact stores — that is a wiring bug at the call
            // site, not a reason to answer from a weaker signal.
            anyhow::bail!(
                "impact is not available on the '{}' backend as constructed — it needs the \
                 metadata store and embedder (IndexBackend::with_impact); probe \
                 supports(StructuralOp::Impact) before calling",
                "index"
            )
        };
        ImpactAnalyzer::new(deps.metadata_store, self.vector_store, deps.embedder)
            .analyze(target, config, depth, repo)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam's contract is object-safety + the reference impl compiling
    /// behind it: a backend can be selected at runtime.
    #[test]
    fn the_seam_is_object_safe() {
        fn _takes_dyn(_b: &mut dyn StructuralBackend) {}
    }
}
