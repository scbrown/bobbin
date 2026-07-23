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

use super::impact::{ImpactConfig, ImpactResult};
use super::refs::{FileSymbols, RefAnalyzer, SymbolRefs};

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
pub struct IndexBackend<'a> {
    refs: RefAnalyzer<'a>,
}

impl<'a> IndexBackend<'a> {
    pub fn new(vector_store: &'a mut crate::storage::VectorStore) -> Self {
        Self {
            refs: RefAnalyzer::new(vector_store),
        }
    }
}

#[async_trait]
impl<'a> StructuralBackend for IndexBackend<'a> {
    fn name(&self) -> &'static str {
        "index"
    }

    async fn find_refs(
        &mut self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SymbolRefs> {
        self.refs
            .find_refs(symbol_name, symbol_type, limit, repo)
            .await
    }

    async fn list_symbols(&mut self, file_path: &str, repo: Option<&str>) -> Result<FileSymbols> {
        self.refs.list_symbols(file_path, repo).await
    }

    async fn impact(
        &mut self,
        _target: &str,
        _config: &ImpactConfig,
        _depth: u32,
        _repo: Option<&str>,
    ) -> Result<Vec<ImpactResult>> {
        // ImpactAnalyzer carries its own store handle and config; routing it
        // through the seam is the next increment (it needs the analyzer's
        // constructor signature untangled from the CLI). Deliberately
        // unimplemented rather than half-wired: an op is either behind the
        // seam or it is not.
        anyhow::bail!(
            "impact is not routed through the structural seam yet — call ImpactAnalyzer directly \
             (seam increment tracked in the swappable-backend work)"
        )
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
