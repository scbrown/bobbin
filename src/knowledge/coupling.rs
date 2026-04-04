//! Push git coupling scores to Quipu as weighted `co_changed_with` edges.
//!
//! For each coupling pair above the threshold, we create:
//! 1. A direct edge: `CodeModule/A --co_changed_with--> CodeModule/B`
//!    (enables graph traversal, PageRank, shortest path)
//! 2. A reified coupling node with score and co-change count
//!    (enables weighted graph algorithms)

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::types::FileCoupling;

const BOBBIN_NS: &str = "https://bobbin.dev/";

/// Push coupling scores to the Quipu knowledge graph as weighted edges.
///
/// Returns `(transaction_id, triple_count)` on success.
pub fn push_coupling_to_quipu(
    couplings: &[FileCoupling],
    repo_name: &str,
    repo_root: &Path,
) -> Result<(i64, usize)> {
    if couplings.is_empty() {
        return Ok((-1, 0));
    }

    let quipu_config = quipu::QuipuConfig::load(repo_root);
    let db_path = if quipu_config.store_path.is_relative() {
        repo_root.join(&quipu_config.store_path)
    } else {
        quipu_config.store_path.clone()
    };

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create quipu store directory")?;
    }

    let mut store = quipu::Store::open(db_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to open quipu store: {e}"))?;

    let turtle = generate_coupling_turtle(couplings, repo_name);

    let timestamp = chrono::Utc::now().to_rfc3339();
    let input = serde_json::json!({
        "turtle": turtle,
        "timestamp": timestamp,
        "actor": "bobbin",
        "source": "coupling-analysis"
    });

    let result = quipu::tool_knot(&mut store, &input)
        .map_err(|e| anyhow::anyhow!("Failed to push coupling to quipu: {e}"))?;

    let tx_id = result["tx_id"].as_i64().unwrap_or(-1);
    let count = result["count"].as_u64().unwrap_or(0) as usize;

    Ok((tx_id, count))
}

/// Generate Turtle RDF for coupling edges.
///
/// For each coupling, produces:
/// - A direct `co_changed_with` edge between the two code modules
/// - A reified `FileCoupling` node with score and co_changes properties
fn generate_coupling_turtle(couplings: &[FileCoupling], repo_name: &str) -> String {
    let mut turtle = String::with_capacity(couplings.len() * 512);

    // Prefixes
    writeln!(turtle, "@prefix bobbin: <{BOBBIN_NS}> .").unwrap();
    writeln!(turtle, "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .").unwrap();
    writeln!(turtle).unwrap();

    for c in couplings {
        let file_a_iri = code_module_iri(repo_name, &c.file_a);
        let file_b_iri = code_module_iri(repo_name, &c.file_b);
        let coupling_iri = coupling_node_iri(repo_name, &c.file_a, &c.file_b);

        // Direct edge for graph traversal
        writeln!(
            turtle,
            "<{file_a_iri}> bobbin:co_changed_with <{file_b_iri}> ."
        )
        .unwrap();

        // Reified coupling node with properties
        writeln!(turtle, "<{coupling_iri}> a bobbin:FileCoupling ;").unwrap();
        writeln!(turtle, "    bobbin:source <{file_a_iri}> ;").unwrap();
        writeln!(turtle, "    bobbin:target <{file_b_iri}> ;").unwrap();
        writeln!(
            turtle,
            "    bobbin:score \"{:.4}\"^^xsd:float ;",
            c.score
        )
        .unwrap();
        writeln!(
            turtle,
            "    bobbin:co_changes \"{}\"^^xsd:integer .",
            c.co_changes
        )
        .unwrap();
        writeln!(turtle).unwrap();
    }

    turtle
}

/// Build the IRI for a code module entity.
fn code_module_iri(repo: &str, path: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}",
        iri_encode(repo),
        iri_encode(path)
    )
}

/// Build a deterministic IRI for a coupling node (enables idempotent upsert).
fn coupling_node_iri(repo: &str, file_a: &str, file_b: &str) -> String {
    format!(
        "{BOBBIN_NS}coupling/{}/{}::{}",
        iri_encode(repo),
        iri_encode(file_a),
        iri_encode(file_b)
    )
}

/// Minimal IRI encoding — encode characters not valid in IRI path segments.
fn iri_encode(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('"', "%22")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_coupling_turtle() {
        let couplings = vec![FileCoupling {
            file_a: "src/search/context.rs".to_string(),
            file_b: "src/cli/context.rs".to_string(),
            score: 0.82,
            co_changes: 15,
            last_co_change: 1700000000,
        }];

        let turtle = generate_coupling_turtle(&couplings, "bobbin");
        assert!(turtle.contains("bobbin:co_changed_with"));
        assert!(turtle.contains("bobbin:FileCoupling"));
        assert!(turtle.contains("0.8200"));
        assert!(turtle.contains("\"15\""));
    }

    #[test]
    fn test_code_module_iri() {
        let iri = code_module_iri("myrepo", "src/main.rs");
        assert_eq!(iri, "https://bobbin.dev/code/myrepo/src/main.rs");
    }

    #[test]
    fn test_coupling_node_iri() {
        let iri = coupling_node_iri("repo", "a.rs", "b.rs");
        assert_eq!(iri, "https://bobbin.dev/coupling/repo/a.rs::b.rs");
    }

    #[test]
    fn test_iri_encode_spaces() {
        assert_eq!(iri_encode("path with spaces"), "path%20with%20spaces");
    }

    #[test]
    fn test_empty_couplings() {
        let turtle = generate_coupling_turtle(&[], "repo");
        // Should just have the prefix declarations
        assert!(turtle.contains("@prefix"));
        assert!(!turtle.contains("co_changed_with"));
    }
}
