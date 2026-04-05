use anyhow::{bail, Context, Result};
use clap::Args;
use std::fmt::Write;
use std::path::PathBuf;

use super::OutputConfig;
use crate::tags::{BundleConfig, BundleRef, RefTarget, TagsConfig};

const BOBBIN_NS: &str = "https://bobbin.dev/";

#[derive(Args)]
pub struct MigrateBundlesArgs {
    /// Directory containing .bobbin/ config
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Preview migration without writing to Quipu or modifying tags.toml
    #[arg(long)]
    dry_run: bool,

    /// Keep bundle definitions in tags.toml after migration (don't remove them)
    #[arg(long)]
    keep: bool,
}

pub async fn run(args: MigrateBundlesArgs, output: OutputConfig) -> Result<()> {
    let repo_root = args.path.canonicalize().unwrap_or(args.path.clone());

    // Find the tags.toml and load it
    let effective_root = find_tags_root(&repo_root).unwrap_or_else(|| repo_root.clone());
    let tags_path = TagsConfig::tags_path(&effective_root);
    let config = TagsConfig::load_or_default(&tags_path);

    if config.bundles.is_empty() {
        if output.json {
            println!(
                "{}",
                serde_json::json!({
                    "migrated": 0,
                    "triples": 0,
                    "message": "No bundles to migrate"
                })
            );
        } else if !output.quiet {
            println!("No bundles found in tags.toml — nothing to migrate.");
        }
        return Ok(());
    }

    let bundle_count = config.bundles.len();

    if !output.quiet && !output.json {
        if args.dry_run {
            println!(
                "Dry run: would migrate {} bundle{} to Quipu",
                bundle_count,
                if bundle_count == 1 { "" } else { "s" }
            );
        } else {
            println!(
                "Migrating {} bundle{} to Quipu...",
                bundle_count,
                if bundle_count == 1 { "" } else { "s" }
            );
        }
    }

    // Detect repo name from git remote or directory name
    let repo_name = detect_repo_name(&effective_root);

    // Generate Turtle RDF for all bundles
    let turtle = generate_bundle_turtle(&config.bundles, &repo_name, &tags_path);

    if output.verbose && !output.json {
        println!("\n--- Generated Turtle RDF ---");
        println!("{turtle}");
        println!("--- End Turtle RDF ---\n");
    }

    if args.dry_run {
        // In dry-run mode, just report what would happen
        let triple_estimate = estimate_triple_count(&config.bundles);
        if output.json {
            let bundle_names: Vec<&str> = config.bundles.iter().map(|b| b.name.as_str()).collect();
            println!(
                "{}",
                serde_json::json!({
                    "dry_run": true,
                    "migrated": bundle_count,
                    "triples_estimate": triple_estimate,
                    "bundles": bundle_names,
                })
            );
        } else if !output.quiet {
            for b in &config.bundles {
                println!("  → {}", b.name);
            }
            println!(
                "\nWould write ~{} triples to Quipu. Run without --dry-run to execute.",
                triple_estimate
            );
        }
        return Ok(());
    }

    // Push to Quipu
    let (tx_id, count) = push_bundles_to_quipu(&turtle, &effective_root)?;

    // Remove bundles from tags.toml (preserving rules, effects, ontology)
    if !args.keep {
        let mut updated_config = config.clone();
        updated_config.bundles.clear();
        updated_config
            .save(&tags_path)
            .context("Failed to save updated tags.toml")?;
    }

    // Report results
    if output.json {
        let bundle_names: Vec<&str> = config.bundles.iter().map(|b| b.name.as_str()).collect();
        println!(
            "{}",
            serde_json::json!({
                "migrated": bundle_count,
                "triples": count,
                "tx_id": tx_id,
                "bundles": bundle_names,
                "tags_toml_updated": !args.keep,
            })
        );
    } else if !output.quiet {
        for b in &config.bundles {
            println!("  ✓ {}", b.name);
        }
        println!(
            "\nMigrated {} bundle{} ({} triples, tx {})",
            bundle_count,
            if bundle_count == 1 { "" } else { "s" },
            count,
            tx_id
        );
        if args.keep {
            println!("Bundle definitions kept in tags.toml (--keep).");
        } else {
            println!("Bundle definitions removed from tags.toml.");
            println!("Tags, effects, and ontology remain unchanged.");
        }
    }

    Ok(())
}

/// Push bundle Turtle RDF to the Quipu knowledge graph.
///
/// Returns `(transaction_id, triple_count)` on success.
#[cfg(feature = "knowledge")]
fn push_bundles_to_quipu(
    turtle: &str,
    repo_root: &std::path::Path,
) -> Result<(i64, usize)> {
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

    let timestamp = chrono::Utc::now().to_rfc3339();
    let input = serde_json::json!({
        "turtle": turtle,
        "timestamp": timestamp,
        "actor": "bobbin",
        "source": "migrate-bundles"
    });

    let result = quipu::tool_knot(&mut store, &input)
        .map_err(|e| anyhow::anyhow!("Failed to push bundles to quipu: {e}"))?;

    let tx_id = result["tx_id"].as_i64().unwrap_or(-1);
    let count = result["count"].as_u64().unwrap_or(0) as usize;

    Ok((tx_id, count))
}

#[cfg(not(feature = "knowledge"))]
fn push_bundles_to_quipu(
    _turtle: &str,
    _repo_root: &std::path::Path,
) -> Result<(i64, usize)> {
    bail!(
        "Knowledge feature not enabled. Rebuild with:\n  \
         cargo build --features knowledge\n\n\
         Or via just:\n  \
         just build --features knowledge"
    )
}

/// Generate Turtle RDF for all bundles.
fn generate_bundle_turtle(
    bundles: &[BundleConfig],
    repo_name: &str,
    tags_path: &std::path::Path,
) -> String {
    let mut turtle = String::with_capacity(bundles.len() * 1024);

    // Prefixes
    writeln!(turtle, "@prefix bobbin: <{BOBBIN_NS}ontology#> .").unwrap();
    writeln!(
        turtle,
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> ."
    )
    .unwrap();
    writeln!(
        turtle,
        "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> ."
    )
    .unwrap();
    writeln!(
        turtle,
        "@prefix prov: <http://www.w3.org/ns/prov#> ."
    )
    .unwrap();
    writeln!(turtle).unwrap();

    let tags_origin = tags_path.display().to_string();

    for bundle in bundles {
        let slug = bundle.slug();
        let this_iri = bundle_iri(&slug);

        // Type assertion
        writeln!(turtle, "<{this_iri}> a bobbin:Bundle ;").unwrap();

        // Label and description
        writeln!(
            turtle,
            "    rdfs:label \"{}\" ;",
            turtle_escape(&bundle.name)
        )
        .unwrap();
        writeln!(
            turtle,
            "    bobbin:description \"{}\" ;",
            turtle_escape(&bundle.description)
        )
        .unwrap();

        // Slug property
        writeln!(
            turtle,
            "    bobbin:slug \"{}\" ;",
            turtle_escape(&slug)
        )
        .unwrap();

        // Keywords
        for kw in &bundle.keywords {
            writeln!(
                turtle,
                "    bobbin:keyword \"{}\" ;",
                turtle_escape(kw)
            )
            .unwrap();
        }

        // contains edges: files → CodeModule
        for file in &bundle.files {
            let (file_repo, file_path) = parse_repo_path(file, repo_name);
            let module_iri = code_module_iri(&file_repo, &file_path);
            writeln!(turtle, "    bobbin:contains <{module_iri}> ;").unwrap();
        }

        // contains edges: docs → CodeModule
        for doc in &bundle.docs {
            let (doc_repo, doc_path) = parse_repo_path(doc, repo_name);
            let module_iri = code_module_iri(&doc_repo, &doc_path);
            writeln!(turtle, "    bobbin:contains <{module_iri}> ;").unwrap();
        }

        // contains edges: refs → CodeModule or CodeSymbol
        for r in &bundle.refs {
            if let Some(parsed) = BundleRef::parse(r) {
                let ref_repo = parsed
                    .repo
                    .as_deref()
                    .unwrap_or(repo_name);
                match &parsed.target {
                    RefTarget::WholeFile => {
                        let iri = code_module_iri(ref_repo, &parsed.file);
                        writeln!(turtle, "    bobbin:contains <{iri}> ;").unwrap();
                    }
                    RefTarget::Symbol(sym) => {
                        let iri = code_symbol_iri(ref_repo, &parsed.file, sym);
                        writeln!(turtle, "    bobbin:contains <{iri}> ;").unwrap();
                    }
                    RefTarget::Heading(heading) => {
                        let iri = section_iri(ref_repo, &parsed.file, heading);
                        writeln!(turtle, "    bobbin:contains <{iri}> ;").unwrap();
                    }
                }
            }
        }

        // includes edges: other bundles
        for inc in &bundle.includes {
            let inc_slug = inc.replace('/', "-");
            let inc_iri = bundle_iri(&inc_slug);
            writeln!(turtle, "    bobbin:includes <{inc_iri}> ;").unwrap();
        }

        // depends_on edges
        for dep in &bundle.depends_on {
            let dep_slug = dep.replace('/', "-");
            let dep_iri = bundle_iri(&dep_slug);
            writeln!(turtle, "    bobbin:depends_on <{dep_iri}> ;").unwrap();
        }

        // implements edges
        for imp in &bundle.implements {
            let imp_slug = imp.replace('/', "-");
            let imp_iri = bundle_iri(&imp_slug);
            writeln!(turtle, "    bobbin:implements <{imp_iri}> ;").unwrap();
        }

        // tests edges
        for test_bundle in &bundle.tests {
            let test_slug = test_bundle.replace('/', "-");
            let test_iri = bundle_iri(&test_slug);
            writeln!(turtle, "    bobbin:tests <{test_iri}> ;").unwrap();
        }

        // Provenance: migrated_from tags.toml
        writeln!(
            turtle,
            "    bobbin:migrated_from \"{}\" .",
            turtle_escape(&tags_origin)
        )
        .unwrap();
        writeln!(turtle).unwrap();
    }

    turtle
}

/// Estimate triple count for dry-run reporting.
fn estimate_triple_count(bundles: &[BundleConfig]) -> usize {
    bundles.iter().map(|b| {
        4 // type + label + description + slug + migrated_from = 5, but we count base as 4
        + 1 // migrated_from
        + b.keywords.len()
        + b.files.len()
        + b.docs.len()
        + b.refs.len()
        + b.includes.len()
        + b.depends_on.len()
        + b.implements.len()
        + b.tests.len()
    }).sum()
}

/// Build IRI for a Bundle entity.
fn bundle_iri(slug: &str) -> String {
    format!("{BOBBIN_NS}bundle/{}", iri_encode(slug))
}

/// Build IRI for a CodeModule entity.
fn code_module_iri(repo: &str, path: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}",
        iri_encode(repo),
        iri_encode(path)
    )
}

/// Build IRI for a CodeSymbol entity.
fn code_symbol_iri(repo: &str, path: &str, symbol: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}::{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(symbol)
    )
}

/// Build IRI for a Section entity (doc heading).
fn section_iri(repo: &str, path: &str, heading: &str) -> String {
    let slug = heading
        .to_lowercase()
        .replace(' ', "-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "");
    format!(
        "{BOBBIN_NS}doc/{}/{}#{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(&slug)
    )
}

/// Parse a `repo:path` or bare `path` into (repo, path).
fn parse_repo_path<'a>(s: &'a str, default_repo: &str) -> (String, String) {
    if let Some((repo, path)) = s.split_once(':') {
        // Only treat as repo:path if the part before ':' looks like a repo name
        // (no slashes, no dots suggesting a file extension)
        if !repo.contains('/') && !repo.contains('.') && !repo.is_empty() {
            return (repo.to_string(), path.to_string());
        }
    }
    (default_repo.to_string(), s.to_string())
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

/// Escape a string for Turtle literal (double-quoted).
fn turtle_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Detect repository name from git remote or directory name.
fn detect_repo_name(repo_root: &std::path::Path) -> String {
    // Try git remote origin
    if let Ok(output) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Extract repo name from URL (last segment, strip .git)
            if let Some(name) = url
                .rsplit('/')
                .next()
                .map(|s| s.trim_end_matches(".git").to_string())
            {
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }

    // Fall back to directory name
    repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Walk up from a starting path to find a directory containing .bobbin/tags.toml.
fn find_tags_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let tags = current.join(".bobbin").join("tags.toml");
        if tags.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_iri() {
        assert_eq!(
            bundle_iri("context-pipeline"),
            "https://bobbin.dev/bundle/context-pipeline"
        );
    }

    #[test]
    fn test_code_module_iri() {
        assert_eq!(
            code_module_iri("bobbin", "src/main.rs"),
            "https://bobbin.dev/code/bobbin/src/main.rs"
        );
    }

    #[test]
    fn test_code_symbol_iri() {
        assert_eq!(
            code_symbol_iri("bobbin", "src/tags.rs", "BundleConfig"),
            "https://bobbin.dev/code/bobbin/src/tags.rs::BundleConfig"
        );
    }

    #[test]
    fn test_section_iri() {
        assert_eq!(
            section_iri("bobbin", "docs/README.md", "Getting Started"),
            "https://bobbin.dev/doc/bobbin/docs/README.md#getting-started"
        );
    }

    #[test]
    fn test_parse_repo_path_with_repo() {
        let (repo, path) = parse_repo_path("aegis:src/main.rs", "default");
        assert_eq!(repo, "aegis");
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn test_parse_repo_path_bare() {
        let (repo, path) = parse_repo_path("src/main.rs", "bobbin");
        assert_eq!(repo, "bobbin");
        assert_eq!(path, "src/main.rs");
    }

    #[test]
    fn test_turtle_escape() {
        assert_eq!(turtle_escape("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(turtle_escape("line\nnewline"), "line\\nnewline");
    }

    #[test]
    fn test_generate_bundle_turtle_basic() {
        let bundles = vec![BundleConfig {
            name: "search/reranking".to_string(),
            description: "Search result reranking pipeline".to_string(),
            keywords: vec!["rerank".to_string(), "scoring".to_string()],
            tags: vec![],
            files: vec!["src/search/rerank.rs".to_string()],
            refs: vec![],
            docs: vec!["docs/reranking.md".to_string()],
            beads: vec![],
            includes: vec!["search/core".to_string()],
            implements: vec![],
            depends_on: vec!["embeddings".to_string()],
            tests: vec![],
            repos: vec![],
            slug: None,
        }];

        let turtle = generate_bundle_turtle(
            &bundles,
            "bobbin",
            std::path::Path::new("/repo/.bobbin/tags.toml"),
        );

        assert!(turtle.contains("bobbin:Bundle"));
        assert!(turtle.contains("search-reranking"));
        assert!(turtle.contains("bobbin:keyword \"rerank\""));
        assert!(turtle.contains("bobbin:keyword \"scoring\""));
        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/code/bobbin/src/search/rerank.rs>"));
        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/code/bobbin/docs/reranking.md>"));
        assert!(turtle.contains("bobbin:includes <https://bobbin.dev/bundle/search-core>"));
        assert!(turtle.contains("bobbin:depends_on <https://bobbin.dev/bundle/embeddings>"));
        assert!(turtle.contains("bobbin:migrated_from"));
    }

    #[test]
    fn test_generate_bundle_turtle_empty() {
        let turtle = generate_bundle_turtle(
            &[],
            "bobbin",
            std::path::Path::new("/repo/.bobbin/tags.toml"),
        );
        assert!(turtle.contains("@prefix"));
        assert!(!turtle.contains("bobbin:Bundle"));
    }

    #[test]
    fn test_estimate_triple_count() {
        let bundles = vec![BundleConfig {
            name: "test".to_string(),
            description: "A test bundle".to_string(),
            keywords: vec!["a".to_string(), "b".to_string()],
            tags: vec![],
            files: vec!["f.rs".to_string()],
            refs: vec![],
            docs: vec![],
            beads: vec![],
            includes: vec![],
            implements: vec![],
            depends_on: vec!["dep".to_string()],
            tests: vec![],
            repos: vec![],
            slug: None,
        }];
        // 4 base + 1 migrated_from + 2 keywords + 1 file + 1 dep = 9
        assert_eq!(estimate_triple_count(&bundles), 9);
    }
}
