//! Tests for the `bobbin ontology` subcommands.
//!
//! Split out of the former `src/cli/ontology.rs` (bobbin-aoz). Sits beside the
//! submodules rather than inside one, because it covers both the inference and
//! the query surfaces.

use super::infer::*;
use super::query::*;
use super::*;

use super::*;
use crate::types::FileCoupling;

fn edge(a: &str, b: &str, score: f32) -> FileCoupling {
    FileCoupling {
        file_a: a.to_string(),
        file_b: b.to_string(),
        score,
        co_changes: 5,
        last_co_change: 0,
    }
}

#[test]
fn test_concept_name_from_paths_common_dir() {
    let files = vec![
        "src/search/hybrid/a.rs".to_string(),
        "src/search/hybrid/b.rs".to_string(),
    ];
    let (name, parent) = concept_name_from_paths(&files);
    assert_eq!(name, "hybrid");
    assert_eq!(parent.as_deref(), Some("search"));
}

#[test]
fn test_concept_name_from_paths_divergent() {
    let files = vec!["src/a.rs".to_string(), "tests/b.rs".to_string()];
    let (name, parent) = concept_name_from_paths(&files);
    assert_eq!(name, "cluster");
    assert_eq!(parent, None);
}

#[test]
fn test_cluster_coupling_components() {
    // Two disjoint communities.
    let edges = vec![
        edge("src/auth/a.rs", "src/auth/b.rs", 0.9),
        edge("src/auth/b.rs", "src/auth/c.rs", 0.8),
        edge("src/db/x.rs", "src/db/y.rs", 0.7),
    ];
    let clusters = cluster_coupling(&edges, 3);
    // auth cluster has 3 files (kept); db cluster has 2 (dropped, < min_size).
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].len(), 3);
}

#[test]
fn test_infer_concepts_names_cluster() {
    let clusters = vec![vec![
        "src/auth/a.rs".to_string(),
        "src/auth/b.rs".to_string(),
        "src/auth/c.rs".to_string(),
    ]];
    let concepts = infer_concepts(&clusters);
    assert_eq!(concepts.len(), 1);
    assert_eq!(concepts[0].name, "auth");
    assert_eq!(concepts[0].members.len(), 3);
}
