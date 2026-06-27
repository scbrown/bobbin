use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::OutputConfig;
use crate::tags::{OntologyConfig, TagsConfig};

#[derive(Args)]
pub struct OntologyArgs {
    #[command(subcommand)]
    command: OntologyCommands,

    /// Directory containing .bobbin/ config
    #[arg(default_value = ".", global = true)]
    path: PathBuf,
}

#[derive(Subcommand)]
enum OntologyCommands {
    /// Show a tag's definition, hierarchy, and relationships
    Show(ShowArgs),
    /// Expand a tag: show all descendants in the hierarchy
    Expand(ExpandArgs),
    /// Find the path between two tags through the hierarchy
    Path(PathArgs),
    /// List all tag definitions in the ontology
    List(ListArgs),
    /// Show the full ontology tree
    Tree(TreeArgs),
    /// Infer candidate ontology concepts from git coupling communities (GH#14 D5)
    Infer(InferArgs),
}

#[derive(Args)]
struct InferArgs {
    /// Minimum coupling score for edges. Default: 0.3
    #[arg(long, default_value = "0.3")]
    threshold: f32,
    /// Minimum cluster size to propose as a concept. Default: 3
    #[arg(long, default_value = "3")]
    min_size: usize,
    /// Max concepts to print. Default: 20
    #[arg(long, default_value = "20")]
    limit: usize,
}

#[derive(Args)]
struct ShowArgs {
    /// Tag name to inspect
    tag: String,
}

#[derive(Args)]
struct ExpandArgs {
    /// Tag name to expand
    tag: String,
}

#[derive(Args)]
struct PathArgs {
    /// Source tag
    from: String,
    /// Target tag
    to: String,
}

#[derive(Args)]
struct ListArgs {
    /// Show only root tags (no parent)
    #[arg(long)]
    roots: bool,
}

#[derive(Args)]
struct TreeArgs {
    /// Root tag to start from (default: show all roots)
    root: Option<String>,
}

pub async fn run(args: OntologyArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args.path.canonicalize().unwrap_or(args.path);
    let config = load_tags_config(&repo_root);

    match args.command {
        OntologyCommands::Show(a) => run_show(&config, &a, &output),
        OntologyCommands::Expand(a) => run_expand(&config, &a, &output),
        OntologyCommands::Path(a) => run_path(&config, &a, &output),
        OntologyCommands::List(a) => run_list(&config, &a, &output),
        OntologyCommands::Tree(a) => run_tree(&config, &a, &output),
        OntologyCommands::Infer(a) => run_infer(&repo_root, &config, &a, &output),
    }
}

/// A candidate ontology concept inferred from a coupling community.
#[derive(Debug, PartialEq)]
struct InferredConcept {
    name: String,
    parent: Option<String>,
    members: Vec<String>,
}

/// Cluster coupling edges into connected components (BFS), keeping components
/// with at least `min_size` files.
fn cluster_coupling(
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
fn concept_name_from_paths(files: &[String]) -> (String, Option<String>) {
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

fn infer_concepts(clusters: &[Vec<String>]) -> Vec<InferredConcept> {
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

fn run_infer(
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

fn load_tags_config(repo_root: &std::path::Path) -> TagsConfig {
    let tags_path = TagsConfig::tags_path(repo_root);
    if tags_path.exists() {
        TagsConfig::load_or_default(&tags_path)
    } else {
        // Try global config
        if let Some(global_dir) = crate::config::Config::global_config_dir() {
            let global_path = global_dir.join("tags.toml");
            if global_path.exists() {
                return TagsConfig::load_or_default(&global_path);
            }
        }
        TagsConfig::default()
    }
}

fn run_show(config: &TagsConfig, args: &ShowArgs, output: &OutputConfig) -> Result<()> {
    let ontology = &config.ontology;

    if ontology.is_empty() {
        bail!("No ontology defined. Add [ontology.tags.*] sections to .bobbin/tags.toml");
    }

    let def = ontology
        .tags
        .get(&args.tag)
        .ok_or_else(|| anyhow::anyhow!("Tag '{}' not found in ontology", args.tag))?;

    if output.json {
        let json = serde_json::json!({
            "tag": args.tag,
            "parent": def.parent,
            "relates_to": def.relates_to,
            "description": def.description,
            "children": ontology.children(&args.tag),
            "ancestors": config.tag_ancestors(&args.tag),
            "related": config.related_tags(&args.tag),
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    println!("Tag: {}", args.tag);
    if let Some(ref desc) = def.description {
        println!("  Description: {}", desc);
    }
    if let Some(ref parent) = def.parent {
        println!("  Parent: {}", parent);
    }

    let ancestors = config.tag_ancestors(&args.tag);
    if !ancestors.is_empty() {
        println!("  Ancestry: {} → {}", ancestors.iter().rev().cloned().collect::<Vec<_>>().join(" → "), args.tag);
    }

    let children = ontology.children(&args.tag);
    if !children.is_empty() {
        println!("  Children: {}", children.join(", "));
    }

    if !def.relates_to.is_empty() {
        println!("  Relates to: {}", def.relates_to.join(", "));
    }

    // Show effect if defined
    if let Some(effect) = config.effects.get(&args.tag) {
        println!("  Effect: boost={}, exclude={}, pin={}", effect.boost, effect.exclude, effect.pin);
    }

    // Show bundles that use this tag
    let tagged_bundles: Vec<&str> = config
        .bundles
        .iter()
        .filter(|b| b.tags.iter().any(|t| t == &args.tag))
        .map(|b| b.name.as_str())
        .collect();
    if !tagged_bundles.is_empty() {
        println!("  Bundles: {}", tagged_bundles.join(", "));
    }

    Ok(())
}

fn run_expand(config: &TagsConfig, args: &ExpandArgs, output: &OutputConfig) -> Result<()> {
    let ontology = &config.ontology;

    if ontology.is_empty() {
        bail!("No ontology defined. Add [ontology.tags.*] sections to .bobbin/tags.toml");
    }

    if !ontology.tags.contains_key(&args.tag) {
        bail!("Tag '{}' not found in ontology", args.tag);
    }

    let descendants = ontology.descendants(&args.tag);

    if output.json {
        let mut expanded = vec![args.tag.clone()];
        expanded.extend(descendants.clone());
        let json = serde_json::json!({
            "tag": args.tag,
            "descendants": descendants,
            "expanded": expanded,
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    println!("Expanding: {}", args.tag);
    if descendants.is_empty() {
        println!("  (no descendants)");
    } else {
        print_tree_from(ontology, &args.tag, 1);
    }

    println!();
    println!("Total: {} tag(s) (1 root + {} descendants)", 1 + descendants.len(), descendants.len());

    Ok(())
}

fn run_path(config: &TagsConfig, args: &PathArgs, output: &OutputConfig) -> Result<()> {
    let ontology = &config.ontology;

    if ontology.is_empty() {
        bail!("No ontology defined. Add [ontology.tags.*] sections to .bobbin/tags.toml");
    }

    // BFS from `from` to `to` through parent/child and relates_to edges
    let path = find_path(ontology, &args.from, &args.to);

    if output.json {
        let json = serde_json::json!({
            "from": args.from,
            "to": args.to,
            "path": path,
            "connected": !path.is_empty(),
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    if path.is_empty() {
        println!("No path found from '{}' to '{}'", args.from, args.to);
    } else {
        println!("{}", path.join(" → "));
    }

    Ok(())
}

fn run_list(config: &TagsConfig, args: &ListArgs, output: &OutputConfig) -> Result<()> {
    let ontology = &config.ontology;

    if ontology.is_empty() {
        bail!("No ontology defined. Add [ontology.tags.*] sections to .bobbin/tags.toml");
    }

    let tags: Vec<&String> = if args.roots {
        let roots = ontology.roots();
        ontology
            .tags
            .keys()
            .filter(|k| roots.contains(&k.to_string()))
            .collect()
    } else {
        let mut keys: Vec<&String> = ontology.tags.keys().collect();
        keys.sort();
        keys
    };

    if output.json {
        let entries: Vec<serde_json::Value> = tags
            .iter()
            .map(|name| {
                let def = &ontology.tags[*name];
                serde_json::json!({
                    "tag": name,
                    "parent": def.parent,
                    "relates_to": def.relates_to,
                    "description": def.description,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    println!("Ontology: {} tag(s)", tags.len());
    println!();
    for name in &tags {
        let def = &ontology.tags[*name];
        let parent_info = def
            .parent
            .as_ref()
            .map(|p| format!(" (parent: {})", p))
            .unwrap_or_default();
        let desc = def
            .description
            .as_ref()
            .map(|d| format!(" — {}", d))
            .unwrap_or_default();
        println!("  {}{}{}", name, parent_info, desc);
    }

    Ok(())
}

fn run_tree(config: &TagsConfig, args: &TreeArgs, output: &OutputConfig) -> Result<()> {
    let ontology = &config.ontology;

    if ontology.is_empty() {
        bail!("No ontology defined. Add [ontology.tags.*] sections to .bobbin/tags.toml");
    }

    if output.json {
        let tree = build_tree_json(ontology, args.root.as_deref());
        println!("{}", serde_json::to_string_pretty(&tree)?);
        return Ok(());
    }

    if let Some(ref root) = args.root {
        if !ontology.tags.contains_key(root) {
            bail!("Tag '{}' not found in ontology", root);
        }
        println!("{}", root);
        print_tree_from(ontology, root, 1);
    } else {
        let mut roots = ontology.roots();
        roots.sort();
        for root in &roots {
            let desc = ontology
                .tags
                .get(root)
                .and_then(|d| d.description.as_ref())
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            println!("{}{}", root, desc);
            print_tree_from(ontology, root, 1);
        }
    }

    Ok(())
}

// === Helpers ===

fn print_tree_from(ontology: &OntologyConfig, tag: &str, depth: usize) {
    let mut children = ontology.children(tag);
    children.sort();
    let indent = "  ".repeat(depth);
    for child in &children {
        let desc = ontology
            .tags
            .get(child)
            .and_then(|d| d.description.as_ref())
            .map(|d| format!(" — {}", d))
            .unwrap_or_default();
        let relates = ontology
            .tags
            .get(child)
            .map(|d| &d.relates_to)
            .filter(|r| !r.is_empty())
            .map(|r| format!(" [relates: {}]", r.join(", ")))
            .unwrap_or_default();
        println!("{}├─ {}{}{}", indent, child, desc, relates);
        print_tree_from(ontology, child, depth + 1);
    }
}

fn find_path(ontology: &OntologyConfig, from: &str, to: &str) -> Vec<String> {
    if from == to {
        return vec![from.to_string()];
    }

    if !ontology.tags.contains_key(from) || !ontology.tags.contains_key(to) {
        return vec![];
    }

    // BFS through parent/child and relates_to edges
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut visited: HashSet<String> = HashSet::new();
    let mut prev: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    visited.insert(from.to_string());
    queue.push_back(from.to_string());

    while let Some(current) = queue.pop_front() {
        // Neighbors: parent, children, relates_to
        let mut neighbors: Vec<String> = Vec::new();

        if let Some(def) = ontology.tags.get(&current) {
            if let Some(ref parent) = def.parent {
                neighbors.push(parent.clone());
            }
            neighbors.extend(def.relates_to.iter().cloned());
        }
        // Children
        for (name, def) in &ontology.tags {
            if def.parent.as_deref() == Some(current.as_str()) {
                neighbors.push(name.clone());
            }
        }

        for neighbor in neighbors {
            if !visited.contains(&neighbor) {
                visited.insert(neighbor.clone());
                prev.insert(neighbor.clone(), current.clone());
                if neighbor == to {
                    // Reconstruct path
                    let mut path = vec![to.to_string()];
                    let mut c = to.to_string();
                    while let Some(p) = prev.get(&c) {
                        path.push(p.clone());
                        c = p.clone();
                    }
                    path.reverse();
                    return path;
                }
                queue.push_back(neighbor);
            }
        }
    }

    vec![] // No path found
}

fn build_tree_json(ontology: &OntologyConfig, root: Option<&str>) -> serde_json::Value {
    fn node_json(ontology: &OntologyConfig, tag: &str) -> serde_json::Value {
        let def = ontology.tags.get(tag);
        let mut children: Vec<String> = ontology.children(tag);
        children.sort();
        let children_json: Vec<serde_json::Value> = children
            .iter()
            .map(|c| node_json(ontology, c))
            .collect();

        serde_json::json!({
            "tag": tag,
            "description": def.and_then(|d| d.description.as_ref()),
            "relates_to": def.map(|d| &d.relates_to).unwrap_or(&vec![]),
            "children": children_json,
        })
    }

    if let Some(root) = root {
        node_json(ontology, root)
    } else {
        let mut roots = ontology.roots();
        roots.sort();
        let trees: Vec<serde_json::Value> = roots.iter().map(|r| node_json(ontology, r)).collect();
        serde_json::json!(trees)
    }
}

#[cfg(test)]
mod infer_tests {
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
}
