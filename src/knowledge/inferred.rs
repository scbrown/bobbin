//! Model/NLP entity extraction — the quarantined track's extractor seam (W3.B).
//!
//! This is the OTHER half of W3.A (`crate::index::entities`, the deterministic
//! track): candidate entities and relationships pulled out of documentation
//! prose by an *inference* step, which the camayoc ingress discipline says may
//! only ever land quarantined — a registered low-trust plane, every fact
//! carrying `quipu:derivedBy` (extractor + params) and
//! `aegis:sourceKind "inferred"`, never masquerading at observed standing.
//! The landing and the masquerade guard live in [`super::quarantine`].
//!
//! ## The seam
//!
//! [`InferredExtractor`] is a pluggable seam, not a model runtime. Bobbin has
//! no generative model today (its only ONNX use is the embedder), so the one
//! implementation here is [`BacktickCoderefExtractor`] — a deterministic
//! heuristic that reads backtick code spans out of markdown prose. It is
//! honestly named for exactly what it does and never pretends to be an LLM;
//! its job is to make the quarantine pipeline real and testable end-to-end.
//! A model-backed extractor becomes one more implementation of this trait
//! later, and inherits the identical discipline: its `id()`/`params()` are
//! recorded as the derivation method, and its output cannot reach the graph
//! any other way than through [`super::quarantine::QuarantinedFacts::stamp`].
//!
//! Even a deterministic heuristic rides the quarantined track when its output
//! is a *guess about meaning* ("this prose refers to that symbol") rather
//! than parser-observed structure — the routing key is epistemic standing,
//! not implementation technique.

use serde::Serialize;

use crate::iri::ONTOLOGY_NS;
use crate::types::Chunk;

/// What kind of thing a candidate entity looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    /// An identifier-shaped reference (`push_chunks_to_quipu`, `foo::bar`).
    Symbol,
    /// A path-shaped reference (`src/knowledge/chunks.rs`).
    Path,
}

/// A candidate entity extracted from prose. A *claim*, not an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateEntity {
    pub name: String,
    pub kind: CandidateKind,
}

/// A candidate relationship: this prose chunk refers to that candidate entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateRelation {
    /// Durable IRI of the source chunk (`crate::iri::chunk_iri` scheme).
    pub chunk_iri: String,
    /// Full predicate IRI the relation claims (e.g. `bobbin:refersTo`).
    pub predicate: String,
    /// Name of the candidate entity referred to (keys into `entities`).
    pub entity_name: String,
}

/// One extractor run's output, prior to quarantine stamping.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Extraction {
    pub entities: Vec<CandidateEntity>,
    pub relations: Vec<CandidateRelation>,
}

impl Extraction {
    /// Fold another batch's output in, deduplicating (the index loop runs
    /// the extractor per file batch and accumulates one run).
    pub fn merge(&mut self, other: Extraction) {
        for e in other.entities {
            if !self.entities.contains(&e) {
                self.entities.push(e);
            }
        }
        for r in other.relations {
            if !self.relations.contains(&r) {
                self.relations.push(r);
            }
        }
    }
}

/// The full IRI of the inferred-track `refersTo` predicate.
pub fn refers_to_predicate() -> String {
    format!("{ONTOLOGY_NS}refersTo")
}

/// The extractor seam for the quarantined (inferred) track.
///
/// Contract: `id()` names the concrete technique honestly (a heuristic is
/// named as a heuristic, a model as a model+revision), `params()` is the
/// complete canonical parameter set needed to reproduce the run, and both are
/// recorded as the `quipu:derivedBy` method on every fact the run produces.
pub trait InferredExtractor {
    /// Stable extractor identity, e.g. `"backtick-coderef/v1"`.
    fn id(&self) -> &str;
    /// Canonical JSON parameters of this configured instance.
    fn params(&self) -> serde_json::Value;
    /// Extract candidates from a repo's chunks. Pure; must not write anywhere.
    fn extract(&self, chunks: &[Chunk], repo: &str) -> Extraction;
}

/// Deterministic baseline extractor: backtick code references in markdown.
///
/// NOT a model. It scans `` `code` `` spans in markdown prose chunks and
/// keeps the identifier- or path-shaped ones as candidate entities, with a
/// `refersTo` candidate relation from the containing chunk. It exists so the
/// quarantined track is exercised end-to-end by real code; a model-backed
/// extractor replaces nothing and simply implements the same trait.
#[derive(Debug, Clone)]
pub struct BacktickCoderefExtractor {
    /// Minimum span length considered a reference (shorter spans are noise).
    pub min_len: usize,
}

impl Default for BacktickCoderefExtractor {
    fn default() -> Self {
        Self { min_len: 3 }
    }
}

impl BacktickCoderefExtractor {
    /// Does a backtick span look like a code reference rather than prose?
    fn classify(&self, span: &str) -> Option<CandidateKind> {
        if span.len() < self.min_len
            || span.chars().any(char::is_whitespace)
            || span.chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        // Path-shaped: contains a separator and no call parens.
        if span.contains('/') && !span.contains('(') {
            return Some(CandidateKind::Path);
        }
        // Identifier-shaped: snake_case, namespaced, dotted, called, or
        // mixed-case single tokens (CamelCase type names).
        let identifierish = span.contains("::")
            || span.contains('_')
            || span.contains("()")
            || span.contains('.')
            || (span.chars().any(|c| c.is_ascii_uppercase())
                && span.chars().any(|c| c.is_ascii_lowercase()));
        let clean = span
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_:.()#'-".contains(c));
        (identifierish && clean).then_some(CandidateKind::Symbol)
    }
}

impl InferredExtractor for BacktickCoderefExtractor {
    fn id(&self) -> &str {
        "backtick-coderef/v1"
    }

    fn params(&self) -> serde_json::Value {
        serde_json::json!({ "min_len": self.min_len })
    }

    fn extract(&self, chunks: &[Chunk], repo: &str) -> Extraction {
        let mut out = Extraction::default();
        let mut seen_entities = std::collections::BTreeSet::new();
        let mut seen_relations = std::collections::BTreeSet::new();

        // Only file-backed markdown prose participates: line 0 marks
        // synthetic sources (commits, beads, SQL rows), same rule as the
        // chunk-graph push and the W3.A producer.
        for chunk in chunks
            .iter()
            .filter(|c| c.language == "markdown" && c.start_line > 0)
        {
            let chunk_iri = crate::iri::chunk_iri(repo, &chunk.file_path, chunk.start_line);
            for span in backtick_spans(&chunk.content) {
                let Some(kind) = self.classify(span) else {
                    continue;
                };
                if seen_entities.insert(span.to_string()) {
                    out.entities.push(CandidateEntity {
                        name: span.to_string(),
                        kind,
                    });
                }
                if seen_relations.insert((chunk_iri.clone(), span.to_string())) {
                    out.relations.push(CandidateRelation {
                        chunk_iri: chunk_iri.clone(),
                        predicate: refers_to_predicate(),
                        entity_name: span.to_string(),
                    });
                }
            }
        }
        out
    }
}

/// Iterate single-backtick spans in markdown text, skipping fenced blocks.
fn backtick_spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            let span = &after[..close];
            if !span.is_empty() {
                spans.push(span);
            }
            rest = &after[close + 1..];
        }
    }
    spans
}
