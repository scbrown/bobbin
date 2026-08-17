//! `bobbin ontology infer` — cluster temporal coupling into candidate concepts.
//!
//! Split out of the former `src/cli/ontology.rs` (bobbin-aoz).

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::cli::OutputConfig;
use crate::tags::{OntologyConfig, TagsConfig};

use super::InferArgs;

/// A candidate ontology concept inferred from a coupling community.
#[derive(Debug, PartialEq)]
pub(super) struct InferredConcept {
    pub(super) name: String,
    pub(super) parent: Option<String>,
    pub(super) members: Vec<String>,
}

/// Cluster coupling edges into connected components (BFS), keeping components
/// with at least `min_size` files.
pub(super) fn cluster_coupling(
    edges: &[crate::types::FileCoupling],
    min_size: usize,
) -> Vec<Vec<String>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for e in edges {
        adj.entry(e.file_a.clone()).or_default().push(e.file_b.clone());
        adj.entry(e.file_b.clone()).or_default().push(e.file_a.clone());
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut clusters: Vec<Vec<String>> = Vec::new();
    // Deterministic iteration order.
    let mut keys: Vec<&String> = adj.keys().collect();
    keys.sort();
    for start in keys {
        if visited.contains(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start.clone());
        visited.insert(start.clone());
        while let Some(cur) = queue.pop_front() {
            component.push(cur.clone());
            if let Some(neis) = adj.get(&cur) {
                for n in neis {
                    if visited.insert(n.clone()) {
                        queue.push_back(n.clone());
                    }
                }
            }
        }
        if component.len() >= min_size {
            component.sort();
            clusters.push(component);
        }
    }
    clusters.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
    clusters
}

/// Derive a concept name + parent from the common directory prefix of files.
/// e.g. files under `src/search/hybrid/*` → (name="hybrid", parent=Some("search")).
pub(super) fn concept_name_from_paths(files: &[String]) -> (String, Option<String>) {
    let dir_components: Vec<Vec<&str>> = files
        .iter()
        .map(|f| {
            let mut parts: Vec<&str> = f.split('/').collect();
            parts.pop(); // drop filename
            parts.into_iter().filter(|p| !p.is_empty()).collect()
        })
        .collect();
    if dir_components.is_empty() {
        return ("cluster".to_string(), None);
    }
    let mut common: Vec<&str> = dir_components[0].clone();
    for dc in &dir_components[1..] {
        let n = common.iter().zip(dc.iter()).take_while(|(a, b)| a == b).count();
        common.truncate(n);
    }
    match common.len() {
        0 => ("cluster".to_string(), None),
        1 => (common[0].to_string(), None),
        n => (common[n - 1].to_string(), Some(common[n - 2].to_string())),
    }
}

pub(super) fn infer_concepts(clusters: &[Vec<String>]) -> Vec<InferredConcept> {
    let mut concepts = Vec::new();
    for (i, cluster) in clusters.iter().enumerate() {
        let (mut name, parent) = concept_name_from_paths(cluster);
        if name == "cluster" {
            name = format!("cluster-{}", i + 1);
        }
        concepts.push(InferredConcept {
            name,
            parent,
            members: cluster.clone(),
        });
    }
    concepts
}

pub(super) fn run_infer(
    repo_root: &std::path::Path,
    config: &TagsConfig,
    args: &InferArgs,
    output: &OutputConfig,
) -> Result<()> {
    let store = crate::storage::MetadataStore::open(&crate::config::Config::db_path(repo_root))?;
    let edges = store.all_coupling(args.threshold, 5000)?;
    if edges.is_empty() {
        if !output.quiet {
            println!(
                "No coupling data above threshold {} — index a repo with git history first.",
                args.threshold
            );
        }
        return Ok(());
    }
    let clusters = cluster_coupling(&edges, args.min_size);
    let concepts: Vec<InferredConcept> = infer_concepts(&clusters)
        .into_iter()
        // Don't re-propose concepts already named in the ontology.
        .filter(|c| !config.ontology.tags.contains_key(&c.name))
        .take(args.limit)
        .collect();

    if output.json {
        let items: Vec<_> = concepts
            .iter()
            .map(|c| serde_json::json!({"name": c.name, "parent": c.parent, "members": c.members}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({"concepts": items}))?);
    } else if !output.quiet {
        if concepts.is_empty() {
            println!(
                "No new concepts inferred (>= {} co-changing files above coupling {}).",
                args.min_size, args.threshold
            );
            return Ok(());
        }
        println!(
            "Inferred {} candidate ontology concept(s) from coupling communities.",
            concepts.len()
        );
        println!("Review and adopt into .bobbin/tags.toml:\n");
        for c in &concepts {
            println!("[ontology.tags.{}]", c.name);
            if let Some(parent) = &c.parent {
                println!("parent = \"{}\"", parent);
            }
            println!("# {} co-changing files:", c.members.len());
            for m in &c.members {
                println!("#   {}", m);
            }
            println!();
        }
    }
    Ok(())
}

