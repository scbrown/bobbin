//! Quarantine landing for inferred facts (W3.B) — the camayoc ingress
//! discipline, enforced in bobbin rather than requested of it.
//!
//! Everything an [`crate::knowledge::inferred::InferredExtractor`] produces
//! lands ONLY in the registered low-trust plane (`crew:inferred`, trust rank
//! 0 in the provenance chain — vocabulary mirrored from camayoc
//! `scripts/planes.py`), every fact carrying `quipu:derivedBy` (extractor +
//! params as the derivation method) and `aegis:sourceKind "inferred"`.
//! Facts reach Turtle exclusively through [`QuarantinedFacts::stamp`]; the
//! type has no other constructor and its fields are private, so an unstamped
//! serialization is unrepresentable. [`validate_inferred_turtle`] re-checks
//! the generated payload anyway (defense in depth against a generator bug)
//! and [`QuarantinedFacts::knot_body`] refuses to build a write from a
//! payload that fails it.
//!
//! ## The landing graph
//!
//! Facts target the camayoc `crew:inferred` plane itself, NOT a private
//! bobbin graph. The plane is registered *and trust-labelled* by camayoc's
//! `scripts/planes.py ensure` (`graph_create` + `graph_label`, rank 0);
//! bobbin deliberately holds no labelling authority, and a fresh graph it
//! minted itself would sit unlabelled — quarantine's appearance without its
//! substance (camayoc-s0h). Bobbin's own replaceable slice of the plane is
//! carved out by the `replace_snapshot` producer key instead, exactly like
//! the chunk-graph push (`super::chunks`).
//!
//! ## The wire and the probe
//!
//! The write is `/knot` with the `graph` param (quipu 22b3569): the target
//! must already be a registered committed graph, unknown IRIs are an error.
//! Older quipu revisions — including the 0.2.0 rev bobbin pinned before the
//! 0.3.23 bump — silently DROP the `graph` key and write to ROOT, which
//! would put inferred facts at observed standing: the one thing this module
//! exists to prevent. [`push_inferred`] therefore probes first with a
//! sentinel unregistered graph; a store that *accepts* that write is a store
//! that ignores routing, and the push refuses. The probe passes against the
//! pinned quipu now — it stays anyway, because it is a correctness guard on
//! whatever store is actually opened, not a workaround for one pin.
//!
//! ## Promotion is not bobbin's
//!
//! Leaving quarantine happens exclusively through camayoc's authority-gated
//! plane promotion (`scripts/promote_plane.py`: stated reason, target must
//! outrank, no self-promotion, human-maintained authority grants). Bobbin
//! never up-tags, re-plane-s, or re-serves its own output at higher trust —
//! a writer that can promote what it wrote has quarantine in name only.
//!
//! ## The envelope rule
//!
//! Any bobbin surface serving these facts stamps plane + trust on the
//! response envelope ([`serve_quarantined`]); serving them bare would let a
//! consumer mistake them for observed facts.

use anyhow::{bail, Context, Result};

use super::inferred::{Extraction, InferredExtractor};
use crate::iri::{iri_encode, CODE_BASE, ONTOLOGY_NS};

/// Trust rank of the inferred plane — the bottom of the provenance chain.
/// Mirrors camayoc `scripts/planes.py` (`crew:inferred`, rank 0).
pub const INFERRED_TRUST_RANK: i64 = 0;

/// Namespace the camayoc planes live under. A parameter, never a hardcoded
/// hostname (camayoc convention); same env override as `planes.py`.
pub fn plane_ns() -> String {
    std::env::var("CAMAYOC_PLANE_NS").unwrap_or_else(|_| "https://camayoc.local/plane/".into())
}

/// Graph IRI of the `crew:inferred` plane — where every inferred fact lands.
pub fn inferred_plane_iri() -> String {
    format!("{}crew/inferred", plane_ns())
}

/// Trust value IRI of the inferred plane (`trust/low`, per planes.py).
pub fn trust_low_iri() -> String {
    format!("{}trust/low", plane_ns())
}

/// The trust chain that makes rank 0 comparable (`chain/provenance`).
pub fn trust_chain_iri() -> String {
    format!("{}chain/provenance", plane_ns())
}

/// The response envelope every surface serving inferred facts must carry.
pub fn envelope() -> serde_json::Value {
    serde_json::json!({
        "plane": inferred_plane_iri(),
        "sourceKind": "inferred",
        "trust": {
            "iri": trust_low_iri(),
            "chain": trust_chain_iri(),
            "rank": INFERRED_TRUST_RANK,
        },
        "standing": "quarantined",
        "promotion": "camayoc authority-gated plane promotion only \
                      (scripts/promote_plane.py); bobbin never promotes its own output",
    })
}

/// Wrap facts for serving. The ONLY sanctioned response shape for inferred
/// facts: envelope first, facts inside — never the facts bare.
pub fn serve_quarantined(facts: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "envelope": envelope(), "facts": facts })
}

/// An extraction stamped with its quarantine markers. Fields are private and
/// [`QuarantinedFacts::stamp`] is the only constructor: inferred candidates
/// cannot reach Turtle without `quipu:derivedBy` + `aegis:sourceKind`.
pub struct QuarantinedFacts {
    extractor_id: String,
    repo: String,
    turtle: String,
}

impl QuarantinedFacts {
    /// Stamp an extraction: serialize every candidate with its derivation
    /// method (extractor id + params) and `aegis:sourceKind "inferred"`.
    pub fn stamp(extractor: &dyn InferredExtractor, extraction: &Extraction, repo: &str) -> Self {
        let base = format!(
            "{CODE_BASE}{}/inferred/{}",
            iri_encode(repo),
            iri_encode(extractor.id())
        );
        let method_iri = format!("{base}/method");
        let mut turtle = String::new();
        turtle.push_str(&format!("@prefix bobbin: <{ONTOLOGY_NS}> .\n"));
        turtle.push_str(&format!("@prefix aegis: <{ONTOLOGY_NS}> .\n"));
        turtle.push_str("@prefix quipu: <https://quipu.dev/ontology/> .\n");
        turtle.push_str("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\n");

        if extraction.entities.is_empty() && extraction.relations.is_empty() {
            return Self {
                extractor_id: extractor.id().to_string(),
                repo: repo.to_string(),
                turtle,
            };
        }

        // The derivation method node: how every fact in this payload can be
        // re-derived. quipu's fact-level vocabulary (system/query/params).
        turtle.push_str(&format!(
            "<{method_iri}> a bobbin:InferredDerivationMethod ;\n"
        ));
        turtle.push_str("    quipu:derivationSystem \"bobbin/inferred-extractor\" ;\n");
        turtle.push_str(&format!(
            "    quipu:derivationQuery \"{}\" ;\n",
            escape_literal(extractor.id())
        ));
        turtle.push_str(&format!(
            "    quipu:derivationParams \"{}\" ;\n",
            escape_literal(&extractor.params().to_string())
        ));
        turtle.push_str("    aegis:sourceKind \"inferred\" .\n\n");

        for entity in &extraction.entities {
            let iri = format!("{base}/{}", iri_encode(&entity.name));
            turtle.push_str(&format!("<{iri}> a bobbin:InferredEntity ;\n"));
            turtle.push_str(&format!(
                "    rdfs:label \"{}\" ;\n",
                escape_literal(&entity.name)
            ));
            turtle.push_str(&format!(
                "    bobbin:candidateKind \"{}\" ;\n",
                match entity.kind {
                    super::inferred::CandidateKind::Symbol => "symbol",
                    super::inferred::CandidateKind::Path => "path",
                }
            ));
            turtle.push_str(&format!("    quipu:derivedBy <{method_iri}> ;\n"));
            turtle.push_str("    aegis:sourceKind \"inferred\" .\n\n");
        }

        // Relations are their own nodes, not direct edges: even inside the
        // quarantine graph an inferred `refersTo` must not be shaped like an
        // observed one, and a reified node carries its own markers.
        for rel in &extraction.relations {
            let rel_iri = format!(
                "{base}/rel/{}-{:016x}",
                iri_encode(&rel.entity_name),
                fnv1a64(rel.chunk_iri.as_bytes())
            );
            let entity_iri = format!("{base}/{}", iri_encode(&rel.entity_name));
            turtle.push_str(&format!("<{rel_iri}> a bobbin:InferredRelation ;\n"));
            turtle.push_str(&format!("    bobbin:relSubject <{}> ;\n", rel.chunk_iri));
            turtle.push_str(&format!("    bobbin:relPredicate <{}> ;\n", rel.predicate));
            turtle.push_str(&format!("    bobbin:relObject <{entity_iri}> ;\n"));
            turtle.push_str(&format!("    quipu:derivedBy <{method_iri}> ;\n"));
            turtle.push_str("    aegis:sourceKind \"inferred\" .\n\n");
        }

        Self {
            extractor_id: extractor.id().to_string(),
            repo: repo.to_string(),
            turtle,
        }
    }

    /// The stamped Turtle payload.
    pub fn turtle(&self) -> &str {
        &self.turtle
    }

    /// The graph these facts are allowed to land in — the inferred plane.
    pub fn graph_iri(&self) -> String {
        inferred_plane_iri()
    }

    /// The `replace_snapshot` producer key: bobbin's replaceable slice of
    /// the plane, per repo and extractor, mirroring the chunk push.
    pub fn snapshot_key(&self) -> String {
        format!("bobbin-inferred:{}:{}", self.repo, self.extractor_id)
    }

    /// The `/knot` body for the quarantine write. Runs the masquerade guard
    /// first and refuses to build a body from an unmarked payload.
    pub fn knot_body(&self, timestamp: &str) -> Result<serde_json::Value> {
        validate_inferred_turtle(&self.turtle)?;
        Ok(serde_json::json!({
            "turtle": self.turtle,
            "timestamp": timestamp,
            "actor": "bobbin",
            "source": format!("inferred-extraction:{}", self.extractor_id),
            "replace_snapshot": true,
            "snapshot": self.snapshot_key(),
            "graph": self.graph_iri(),
        }))
    }
}

/// Masquerade guard: refuse any inferred payload whose subjects are not all
/// marked. Assumes the house serialization (one subject per blank-line
/// block); the sealed [`QuarantinedFacts`] type is the primary guard, this
/// is the tripwire behind it.
pub fn validate_inferred_turtle(turtle: &str) -> Result<()> {
    for block in turtle.split("\n\n") {
        let block = block.trim();
        if block.is_empty() || block.starts_with("@prefix") {
            continue;
        }
        if !block.contains("aegis:sourceKind \"inferred\"") {
            bail!(
                "masquerade refused: inferred payload has a subject without \
                 aegis:sourceKind \"inferred\" — it would serve at observed standing. \
                 Block: {}",
                block.lines().next().unwrap_or("")
            );
        }
        let is_method = block.contains("a bobbin:InferredDerivationMethod");
        if !is_method && !block.contains("quipu:derivedBy") {
            bail!(
                "masquerade refused: inferred payload has a subject without \
                 quipu:derivedBy — its extractor and params would be unrecorded. \
                 Block: {}",
                block.lines().next().unwrap_or("")
            );
        }
    }
    Ok(())
}

/// Push a stamped extraction into the quarantine plane of an open store.
///
/// Probes graph routing FIRST: a `/knot` aimed at a deliberately
/// unregistered sentinel graph must be REFUSED by the store. A store that
/// accepts it is silently dropping the `graph` key (quipu predating
/// 22b3569 — older than the 0.3.23 rev bobbin pins), and the facts would
/// land in ROOT at observed standing; this function refuses instead of
/// writing. The pinned quipu enforces routing, so the probe passes there;
/// it remains as a correctness guard on whatever store is opened.
/// Returns `(tx_id, fact_count)`.
pub fn push_inferred(store: &mut quipu::Store, facts: &QuarantinedFacts) -> Result<(i64, usize)> {
    let sentinel = format!("{}probe/unregistered-sentinel", plane_ns());
    let probe = quipu::tool_knot(
        store,
        &serde_json::json!({
            "turtle": "",
            "actor": "bobbin",
            "source": "inferred-graph-probe",
            "graph": sentinel,
        }),
    );
    match probe {
        Ok(_) => bail!(
            "embedded quipu ACCEPTED a write aimed at an unregistered sentinel graph, \
             so it is silently dropping the /knot 'graph' key: inferred facts would land \
             in ROOT at observed standing — the masquerade the camayoc ingress discipline \
             forbids. Refusing to push. Requires a quipu with strict graph routing \
             (>= 22b3569); the pinned quipu (0.3.23, rev 37bfc06a) has it, so this \
             store was opened by something older."
        ),
        Err(e) if e.to_string().contains("unknown graph") => {} // routing enforced
        Err(e) => bail!("quipu graph-routing probe failed: {e}"),
    }

    let body = facts.knot_body(&chrono::Utc::now().to_rfc3339())?;
    let result = quipu::tool_knot(store, &body).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unknown graph") || msg.contains("not registered") {
            anyhow::anyhow!(
                "quarantine plane {} is not registered: register AND trust-label it via \
                 camayoc `scripts/planes.py ensure` (graph_create + graph_label, rank 0). \
                 Bobbin deliberately holds no labelling authority. Store said: {msg}",
                facts.graph_iri()
            )
        } else {
            anyhow::anyhow!("quarantine push failed: {msg}")
        }
    })?;

    // With quipu's shacl feature compiled in (this build), a validation
    // refusal comes back as Ok with `conforms: false` — refuse, don't
    // report it as a landed write.
    if result.get("conforms").and_then(|v| v.as_bool()) == Some(false) {
        bail!("quarantine push refused by SHACL validation: {result}");
    }

    Ok((
        result["tx_id"].as_i64().unwrap_or(-1),
        result["count"].as_u64().unwrap_or(0) as usize,
    ))
}

/// Open the repo's embedded store and push, mirroring
/// [`super::chunks::push_chunks_to_quipu`]'s open path.
pub fn push_inferred_to_quipu(
    facts: &QuarantinedFacts,
    repo_root: &std::path::Path,
) -> Result<(i64, usize)> {
    let quipu_config = quipu::QuipuConfig::load(repo_root);
    let db_path = if quipu_config.store_path.is_relative() {
        repo_root.join(&quipu_config.store_path)
    } else {
        quipu_config.store_path.clone()
    };
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create quipu store directory")?;
    }
    let mut store = quipu::Store::open(db_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to open quipu store: {e}"))?;
    push_inferred(&mut store, facts)
}

fn escape_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// FNV-1a 64 — deterministic relation-IRI disambiguator (same digest quipu
/// uses for statement IRIs in `derivation.rs`).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
