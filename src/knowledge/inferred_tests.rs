//! Tests for the quarantined inferred-extraction track (W3.B): the extractor
//! seam, the quarantine payload markers, the envelope rule, and the
//! masquerade guard's refusal path. No ONNX, no live quipu server — payload
//! assertions only, per the chunks.rs house pattern.

use super::inferred::*;
use super::quarantine::*;
use crate::types::{Chunk, ChunkType};

fn md_chunk(file: &str, start: u32, content: &str) -> Chunk {
    Chunk {
        id: format!("{file}:{start}"),
        file_path: file.to_string(),
        chunk_type: ChunkType::Section,
        name: Some("Section".to_string()),
        start_line: start,
        end_line: start + 10,
        content: content.to_string(),
        language: "markdown".to_string(),
        tags: String::new(),
    }
}

fn fixture_extraction() -> (BacktickCoderefExtractor, Extraction) {
    let chunks = vec![md_chunk(
        "docs/guide.md",
        3,
        "Call `push_chunks_to_quipu` (see `src/knowledge/chunks.rs`). \
         The `Store` type helps. Not `a b` and not `it`.",
    )];
    let ex = BacktickCoderefExtractor::default();
    let extraction = ex.extract(&chunks, "myrepo");
    (ex, extraction)
}

// ── the extractor seam ─────────────────────────────────────────────

#[test]
fn extractor_finds_code_refs_in_markdown_prose() {
    let (_, extraction) = fixture_extraction();
    let names: Vec<&str> = extraction
        .entities
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(names.contains(&"push_chunks_to_quipu"));
    assert!(names.contains(&"src/knowledge/chunks.rs"));
    assert!(names.contains(&"Store")); // mixed-case type name
    assert!(!names.contains(&"a b")); // whitespace → prose, not a ref
    assert!(!names.contains(&"it")); // too short

    let path = extraction
        .entities
        .iter()
        .find(|e| e.name == "src/knowledge/chunks.rs")
        .unwrap();
    assert_eq!(path.kind, CandidateKind::Path);

    // Relations tie the DURABLE chunk IRI to each candidate by name.
    let rel = extraction
        .relations
        .iter()
        .find(|r| r.entity_name == "push_chunks_to_quipu")
        .unwrap();
    assert_eq!(
        rel.chunk_iri,
        "http://aegis.gastown.local/ontology/chunk/myrepo/docs%2Fguide.md#C3"
    );
    assert_eq!(
        rel.predicate,
        "http://aegis.gastown.local/ontology/refersTo"
    );
}

#[test]
fn extractor_skips_non_markdown_synthetic_and_fenced() {
    let mut rs = md_chunk("src/a.rs", 5, "uses `some_fn` here");
    rs.language = "rust".to_string();
    let synthetic = md_chunk("beads:rig:x", 0, "closed `bd_close` bead");
    let fenced = md_chunk("docs/f.md", 1, "```\n`inside_fence`\n```\nprose only");
    let ex = BacktickCoderefExtractor::default();
    let extraction = ex.extract(&[rs, synthetic, fenced], "r");
    assert!(extraction.entities.is_empty(), "{:?}", extraction.entities);
    assert!(extraction.relations.is_empty());
}

#[test]
fn extractor_identity_is_honest_and_recorded() {
    let ex = BacktickCoderefExtractor::default();
    // The seam contract: the id names the technique, not a model.
    assert_eq!(ex.id(), "backtick-coderef/v1");
    assert_eq!(ex.params(), serde_json::json!({ "min_len": 3 }));
}

// ── the quarantine landing payload ────────────────────────────────

#[test]
fn payload_targets_the_inferred_plane_with_snapshot_replacement() {
    let (ex, extraction) = fixture_extraction();
    let facts = QuarantinedFacts::stamp(&ex, &extraction, "myrepo");
    let body = facts.knot_body("2026-08-22T00:00:00Z").unwrap();

    assert_eq!(body["graph"], "https://camayoc.local/plane/crew/inferred");
    assert_eq!(body["replace_snapshot"], true);
    assert_eq!(
        body["snapshot"],
        "bobbin-inferred:myrepo:backtick-coderef/v1"
    );
    assert_eq!(body["actor"], "bobbin");
    assert_eq!(body["source"], "inferred-extraction:backtick-coderef/v1");
}

#[test]
fn every_fact_carries_derived_by_and_source_kind() {
    let (ex, extraction) = fixture_extraction();
    let facts = QuarantinedFacts::stamp(&ex, &extraction, "myrepo");
    let turtle = facts.turtle();

    // The derivation method records extractor + params.
    assert!(turtle.contains("quipu:derivationQuery \"backtick-coderef/v1\""));
    assert!(turtle.contains("quipu:derivationParams \"{\\\"min_len\\\":3}\""));

    // Every non-method subject block carries BOTH markers.
    let subject_blocks: Vec<&str> = turtle
        .split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty() && !b.starts_with("@prefix"))
        .collect();
    assert!(
        subject_blocks.len() > 2,
        "expected method + entities + relations"
    );
    for block in &subject_blocks {
        assert!(
            block.contains("aegis:sourceKind \"inferred\""),
            "unmarked block: {block}"
        );
        if !block.contains("a bobbin:InferredDerivationMethod") {
            assert!(block.contains("quipu:derivedBy"), "no derivedBy: {block}");
        }
    }

    // And the guard agrees.
    validate_inferred_turtle(turtle).unwrap();
}

#[test]
fn relations_are_reified_not_direct_edges() {
    let (ex, extraction) = fixture_extraction();
    let facts = QuarantinedFacts::stamp(&ex, &extraction, "myrepo");
    let turtle = facts.turtle();
    // The inferred refersTo never appears as a bare chunk→entity edge; it is
    // carried by an InferredRelation node with its own markers.
    assert!(turtle.contains("a bobbin:InferredRelation"));
    assert!(turtle.contains("bobbin:relPredicate <http://aegis.gastown.local/ontology/refersTo>"));
    assert!(!turtle.contains("/C3> bobbin:refersTo"));
    assert!(!turtle.contains("/C3> <http://aegis.gastown.local/ontology/refersTo>"));
}

// ── the masquerade guard's refusal path ───────────────────────────

#[test]
fn guard_refuses_turtle_missing_source_kind() {
    let bare = "@prefix bobbin: <http://aegis.gastown.local/ontology/> .\n\n\
                <http://ex/e1> a bobbin:InferredEntity ;\n    rdfs:label \"x\" .\n";
    let err = validate_inferred_turtle(bare).unwrap_err().to_string();
    assert!(err.contains("masquerade refused"), "{err}");
    assert!(err.contains("sourceKind"), "{err}");
}

#[test]
fn guard_refuses_turtle_missing_derived_by() {
    let unattributed =
        "<http://ex/e1> a bobbin:InferredEntity ;\n    aegis:sourceKind \"inferred\" .\n";
    let err = validate_inferred_turtle(unattributed)
        .unwrap_err()
        .to_string();
    assert!(err.contains("derivedBy"), "{err}");
}

#[test]
fn guard_accepts_the_empty_extraction() {
    let ex = BacktickCoderefExtractor::default();
    let facts = QuarantinedFacts::stamp(&ex, &Extraction::default(), "r");
    // Prefixes only, no subjects: valid, and the knot body still targets
    // the plane (an empty replace retracts a vanished extraction).
    let body = facts.knot_body("2026-08-22T00:00:00Z").unwrap();
    assert_eq!(body["graph"], inferred_plane_iri());
}

// ── the envelope rule ─────────────────────────────────────────────

#[test]
fn served_facts_are_always_wrapped_in_the_quarantine_envelope() {
    let out = serve_quarantined(serde_json::json!({ "n": 1 }));
    let env = &out["envelope"];
    assert_eq!(env["plane"], "https://camayoc.local/plane/crew/inferred");
    assert_eq!(env["sourceKind"], "inferred");
    assert_eq!(env["trust"]["rank"], 0);
    assert_eq!(env["trust"]["iri"], "https://camayoc.local/plane/trust/low");
    assert_eq!(
        env["trust"]["chain"],
        "https://camayoc.local/plane/chain/provenance"
    );
    assert_eq!(env["standing"], "quarantined");
    // Promotion is named as camayoc's, never bobbin's.
    assert!(env["promotion"].as_str().unwrap().contains("promote_plane"));
    assert_eq!(out["facts"]["n"], 1);
}

// ── what the quipu 0.3.23 pin unlocked, verified in-process ────────
//
// These run against `quipu::Store::open_in_memory()` — the same crate the
// production path links — so they are proof the pinned rev actually has the
// behavior the probes in `quarantine.rs`/`chunks.rs` demand, not a mock of it.

#[test]
fn push_inferred_lands_in_the_registered_plane_with_routing_enforced() {
    let mut store = quipu::Store::open_in_memory().expect("in-memory store");
    // Register the plane as a committed graph (camayoc's planes.py does this
    // in production; trust-labelling stays camayoc's and is not needed for
    // routing). The sentinel probe inside push_inferred must be REFUSED by
    // the store first — a store that accepted it would fail this test.
    quipu::tool_graph_create(
        &store,
        &serde_json::json!({ "graph": inferred_plane_iri() }),
    )
    .expect("register inferred plane");

    let (ex, extraction) = fixture_extraction();
    let facts = QuarantinedFacts::stamp(&ex, &extraction, "myrepo");
    let (tx, count) = push_inferred(&mut store, &facts).expect("routed push");
    assert!(tx > 0, "expected a real transaction, got {tx}");
    assert!(count > 0, "expected facts written, got {count}");

    // The facts are IN the plane graph, not ROOT: strict routing means the
    // masquerade (inferred facts at observed standing) is unrepresentable.
    let g = store
        .lookup(&inferred_plane_iri())
        .unwrap()
        .expect("plane interned");
    let in_plane = store.current_facts_in_graph(g).unwrap();
    assert!(!in_plane.is_empty(), "plane graph should hold the facts");
    let in_root = store.current_facts().unwrap();
    assert!(
        in_root.is_empty(),
        "ROOT must stay empty — {} facts leaked to observed standing",
        in_root.len()
    );
}

#[test]
fn push_inferred_refuses_an_unregistered_plane_by_name() {
    let mut store = quipu::Store::open_in_memory().expect("in-memory store");
    // No graph_create: the plane is unknown. The routing probe passes (the
    // sentinel is refused), but the real write must fail with the remedy.
    let (ex, extraction) = fixture_extraction();
    let facts = QuarantinedFacts::stamp(&ex, &extraction, "myrepo");
    let err = push_inferred(&mut store, &facts).unwrap_err().to_string();
    assert!(
        err.contains("planes.py"),
        "error must name camayoc's registration flow, got: {err}"
    );
}

#[test]
fn shacl_gate_is_compiled_in_and_refuses_a_violating_write() {
    // KNOWLEDGE_SHACL_ENABLED (src/mcp/server.rs) reports `true` since the
    // pin bump; this is the test that keeps that report honest. With quipu's
    // `shacl` feature compiled OUT, this write would be accepted unvalidated
    // and the assertion below would fail — a compiled-out gate is
    // indistinguishable from a passed one, except here.
    let mut store = quipu::Store::open_in_memory().expect("in-memory store");
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
         @prefix bobbin: <{ns}> .\n\
         bobbin:ChunkShape a sh:NodeShape ;\n\
             sh:targetClass bobbin:Chunk ;\n\
             sh:property [ sh:path bobbin:filePath ; sh:minCount 1 ] .\n",
        ns = crate::iri::ONTOLOGY_NS
    );
    let violating = format!(
        "@prefix bobbin: <{ns}> .\n<http://ex/c1> a bobbin:Chunk .\n",
        ns = crate::iri::ONTOLOGY_NS
    );
    let out = quipu::tool_knot(
        &mut store,
        &serde_json::json!({
            "turtle": violating,
            "shapes": shapes,
            "timestamp": "2026-08-22T00:00:00Z",
            "actor": "test",
            "source": "shacl-gate-test",
        }),
    )
    .expect("refusal surfaces as Ok(conforms:false), not Err");
    assert_eq!(
        out["conforms"], false,
        "SHACL gate must refuse a bobbin:Chunk without filePath: {out}"
    );
    assert!(
        out.get("tx_id").is_none(),
        "a refused write has no tx: {out}"
    );
}
