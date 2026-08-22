//! Push the chunk graph to Quipu as durable `bobbin:Chunk` facts (W2.P4).
//!
//! Chunk IRIs derive from STABLE COORDINATES — repo + path + `C{start_line}`
//! ordinal, matching the live ingest lane's `S{line}` section idiom — never
//! from bobbin's internal chunk ids, which are content-coordinate hashes
//! that re-mint on any line shift. The graph is the index, never the
//! warehouse: no chunk content crosses over, only identity, membership
//! (`bobbin:inDocument`), order (`bobbin:chunkOrder`), adjacency
//! (`bobbin:nextChunk`), and weak symbol-mention literals
//! (`bobbin:mentions "Name"`, W2.P5 — resolved to Ref edges by the
//! idempotent pass in `knowledge::mentions`). Containment (`part_of`)
//! stays in bobbin's own
//! edges table for now — the graph's `inDocument` covers the
//! chunk→document half the competency questions need.
//!
//! Writes go through `/knot` with `replace_snapshot` under a per-repo
//! producer key, so a reindex is a diffed replace: unchanged facts stay
//! live, vanished chunks retract. The embedded quipu revision is PROBED
//! for snapshot support before any fact is written — a store that predates
//! `replace_snapshot` would silently ignore the key and accumulate a copy
//! of the graph per index run, which is exactly the class of silent misses
//! this integration keeps finding (bobbin-jdlkh).

use std::path::Path;

use anyhow::{Context, Result};

use crate::iri::ONTOLOGY_NS;
use crate::types::{Chunk, ChunkEdge, ChunkEdgeType};

/// Build the durable IRI for a chunk from stable coordinates.
pub(crate) use crate::iri::chunk_iri;

/// Generate Turtle for the chunk graph of one repo. Pure, unit-testable.
///
/// Only file-backed chunks participate (line 0 marks synthetic sources —
/// commits, beads, SQL rows — which have their own identity schemes).
pub(crate) fn generate_chunk_turtle(chunks: &[Chunk], edges: &[ChunkEdge], repo: &str) -> String {
    let mut turtle = String::with_capacity(chunks.len() * 256);
    turtle.push_str(&format!("@prefix bobbin: <{ONTOLOGY_NS}> .\n"));
    turtle.push_str("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n");
    turtle.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n");

    // Map bobbin's volatile chunk ids to durable IRIs for edge emission.
    let mut iri_of: std::collections::HashMap<&str, String> = std::collections::HashMap::new();

    // Weak symbol-mention literals (W2.P5): source-chunk id → mentioned
    // symbol names, from the parser's symbol-bearing edges. The reconcile
    // pass (`knowledge::mentions`) later resolves these to Ref edges.
    let edge_mentions = super::mentions::edge_mention_map(edges);

    let mut file_chunks: Vec<&Chunk> = chunks.iter().filter(|c| c.start_line > 0).collect();
    file_chunks.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
            .then(b.end_line.cmp(&a.end_line))
    });

    let mut order_in_file = 0u32;
    let mut prev_file: Option<&str> = None;
    for chunk in &file_chunks {
        if prev_file != Some(chunk.file_path.as_str()) {
            order_in_file = 0;
            prev_file = Some(chunk.file_path.as_str());
        }
        let iri = chunk_iri(repo, &chunk.file_path, chunk.start_line);
        let module = crate::iri::code_module_iri(repo, &chunk.file_path);
        let label = chunk
            .name
            .clone()
            .unwrap_or_else(|| format!("{}:{}", chunk.file_path, chunk.start_line));

        turtle.push_str(&format!("<{iri}> a bobbin:Chunk ;\n"));
        turtle.push_str(&format!("    bobbin:inDocument <{module}> ;\n"));
        turtle.push_str(&format!(
            "    bobbin:chunkOrder \"{order_in_file}\"^^xsd:integer ;\n"
        ));
        turtle.push_str(&format!(
            "    bobbin:filePath \"{}\" ;\n",
            escape_literal(&chunk.file_path)
        ));
        for name in super::mentions::chunk_mention_names(chunk, &edge_mentions) {
            turtle.push_str(&format!(
                "    bobbin:mentions \"{}\" ;\n",
                escape_literal(name)
            ));
        }
        turtle.push_str(&format!(
            "    rdfs:label \"{}\" .\n\n",
            escape_literal(&label)
        ));

        iri_of.insert(chunk.id.as_str(), iri);
        order_in_file += 1;
    }

    for edge in edges {
        if edge.edge_type != ChunkEdgeType::NextChunk {
            continue;
        }
        let (Some(src), Some(tgt)) = (
            iri_of.get(edge.source_chunk.as_str()),
            iri_of.get(edge.target_chunk.as_str()),
        ) else {
            continue;
        };
        turtle.push_str(&format!("<{src}> bobbin:nextChunk <{tgt}> .\n"));
    }

    turtle
}

fn escape_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Push the chunk graph as a diffed snapshot replacement.
///
/// Returns `(tx_id, fact_count)`. Errors — without writing anything — if
/// the embedded quipu store predates `replace_snapshot`.
pub fn push_chunks_to_quipu(
    chunks: &[Chunk],
    edges: &[ChunkEdge],
    repo_name: &str,
    repo_root: &Path,
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

    let snapshot_key = format!("bobbin-chunks:{repo_name}");

    // Probe snapshot support with an EMPTY payload before writing facts: a
    // quipu that predates replace_snapshot ignores the key (no "replaced"
    // field in the response) and would accumulate one copy of the graph per
    // index run. The probe itself replaces nothing on a supporting store
    // (empty diff) and writes nothing on an old one.
    let probe = quipu::tool_knot(
        &mut store,
        &serde_json::json!({
            "turtle": "",
            "actor": "bobbin",
            "source": "chunk-index-probe",
            "replace_snapshot": true,
            "snapshot": snapshot_key,
        }),
    )
    .map_err(|e| anyhow::anyhow!("Quipu snapshot probe failed: {e}"))?;
    if probe.get("replaced").and_then(|v| v.as_bool()) != Some(true) {
        anyhow::bail!(
            "embedded quipu predates snapshot replacement (no 'replaced' in /knot response); \
             refusing to push chunks — they would accumulate per index run. \
             Update the quipu dependency pin (bobbin-di7)."
        );
    }

    let turtle = generate_chunk_turtle(chunks, edges, repo_name);
    let result = quipu::tool_knot(
        &mut store,
        &serde_json::json!({
            "turtle": turtle,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin",
            "source": "chunk-index",
            "replace_snapshot": true,
            "snapshot": snapshot_key,
        }),
    )
    .map_err(|e| anyhow::anyhow!("Failed to push chunks to quipu: {e}"))?;

    Ok((
        result["tx_id"].as_i64().unwrap_or(-1),
        result["count"].as_u64().unwrap_or(0) as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChunkType;

    fn chunk(id: &str, file: &str, start: u32, name: Option<&str>) -> Chunk {
        Chunk {
            id: id.to_string(),
            file_path: file.to_string(),
            chunk_type: ChunkType::Section,
            name: name.map(str::to_string),
            start_line: start,
            end_line: start + 5,
            content: "body".to_string(),
            language: "markdown".to_string(),
            tags: String::new(),
        }
    }

    #[test]
    fn turtle_uses_stable_coordinates_not_chunk_ids() {
        let chunks = vec![
            chunk("hash1", "docs/guide.md", 1, Some("Intro")),
            chunk("hash2", "docs/guide.md", 7, Some("Intro > Setup")),
        ];
        let edges = vec![ChunkEdge {
            source_chunk: "hash1".to_string(),
            target_chunk: "hash2".to_string(),
            source_name: "Intro".to_string(),
            target_name: "Intro > Setup".to_string(),
            edge_type: ChunkEdgeType::NextChunk,
            file_path: "docs/guide.md".to_string(),
        }];
        let turtle = generate_chunk_turtle(&chunks, &edges, "myrepo");

        // Durable IRIs: repo + %2F-encoded path + C{start_line} ordinal.
        assert!(turtle.contains(
            "<http://aegis.gastown.local/code/myrepo/docs%2Fguide.md/C1> a bobbin:Chunk"
        ));
        assert!(turtle.contains(
            "<http://aegis.gastown.local/code/myrepo/docs%2Fguide.md/C1> bobbin:nextChunk \
             <http://aegis.gastown.local/code/myrepo/docs%2Fguide.md/C7> ."
        ));
        // Membership points at the module/document entity the live ingest
        // lane mints, so the graphs join.
        assert!(turtle.contains(
            "bobbin:inDocument <http://aegis.gastown.local/code/myrepo/docs%2Fguide.md>"
        ));
        // The volatile hash ids never reach the graph.
        assert!(!turtle.contains("hash1"));
    }

    #[test]
    fn synthetic_line_zero_chunks_are_skipped() {
        let chunks = vec![chunk("c", "beads:rig:x", 0, None)];
        let turtle = generate_chunk_turtle(&chunks, &[], "r");
        assert!(!turtle.contains("beads"));
    }

    #[test]
    fn order_restarts_per_file() {
        let chunks = vec![
            chunk("a", "a.md", 1, None),
            chunk("b", "a.md", 9, None),
            chunk("c", "b.md", 3, None),
        ];
        let turtle = generate_chunk_turtle(&chunks, &[], "r");
        let c_block = turtle.split("b.md/C3").nth(1).unwrap();
        assert!(c_block.contains("chunkOrder \"0\""));
    }
}
