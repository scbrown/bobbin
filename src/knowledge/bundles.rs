//! Quipu-backed bundle CRUD operations.
//!
//! When the `knowledge` feature is enabled, bundle commands write to Quipu's
//! RDF graph instead of tags.toml. Each bundle becomes a `bobbin:Bundle` entity
//! with `contains`, `includes`, and `depends_on` edges to member entities.

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::tags::{BundleConfig, BundleRef, RefTarget};

const BOBBIN_NS: &str = "https://bobbin.dev/";

// ---------------------------------------------------------------------------
// Quipu store helpers
// ---------------------------------------------------------------------------

/// Open the Quipu store for the given repo root.
pub fn open_quipu_store(repo_root: &Path) -> Result<quipu::Store> {
    let quipu_config = quipu::QuipuConfig::load(repo_root);
    let db_path = if quipu_config.store_path.is_relative() {
        repo_root.join(&quipu_config.store_path)
    } else {
        quipu_config.store_path.clone()
    };

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).context("creating quipu store directory")?;
    }

    quipu::Store::open(db_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to open quipu store: {e}"))
}

// ---------------------------------------------------------------------------
// IRI construction
// ---------------------------------------------------------------------------

/// Build IRI for a bundle entity.
pub fn bundle_iri(name: &str) -> String {
    format!("{BOBBIN_NS}bundle/{}", iri_encode(name))
}

/// Build IRI for a code module (file).
fn code_module_iri(repo: &str, path: &str) -> String {
    format!("{BOBBIN_NS}code/{}/{}", iri_encode(repo), iri_encode(path))
}

/// Build IRI for a code symbol (file::symbol).
fn code_symbol_iri(repo: &str, path: &str, symbol: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}::{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(symbol)
    )
}

/// Build IRI for a document.
fn document_iri(repo: &str, path: &str) -> String {
    format!("{BOBBIN_NS}doc/{}/{}", iri_encode(repo), iri_encode(path))
}

/// Build IRI for a document section (heading).
fn section_iri(repo: &str, path: &str, heading: &str) -> String {
    format!(
        "{BOBBIN_NS}doc/{}/{}#{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(heading)
    )
}

/// Build member IRI from a BundleRef.
fn member_iri_from_ref(repo: &str, bundle_ref: &BundleRef) -> String {
    let effective_repo = bundle_ref.repo.as_deref().unwrap_or(repo);
    match &bundle_ref.target {
        RefTarget::WholeFile => code_module_iri(effective_repo, &bundle_ref.file),
        RefTarget::Symbol(sym) => code_symbol_iri(effective_repo, &bundle_ref.file, sym),
        RefTarget::Heading(heading) => section_iri(effective_repo, &bundle_ref.file, heading),
    }
}

/// Build member IRI for a file path (may be a doc or code module).
fn member_iri_for_file(repo: &str, path: &str) -> String {
    code_module_iri(repo, path)
}

/// Build member IRI for a doc path.
fn member_iri_for_doc(repo: &str, path: &str) -> String {
    document_iri(repo, path)
}

/// Minimal IRI encoding.
fn iri_encode(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('"', "%22")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

// ---------------------------------------------------------------------------
// Turtle generation
// ---------------------------------------------------------------------------

/// Standard prefixes for bundle Turtle documents.
fn turtle_prefixes() -> String {
    let mut s = String::new();
    writeln!(s, "@prefix bobbin: <{BOBBIN_NS}> .").unwrap();
    writeln!(s, "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .").unwrap();
    writeln!(s, "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .").unwrap();
    writeln!(s).unwrap();
    s
}

/// Generate Turtle RDF for creating a bundle entity with all its edges.
pub fn generate_create_turtle(bundle: &BundleConfig, repo_name: &str) -> String {
    let mut turtle = turtle_prefixes();
    let biri = bundle_iri(&bundle.name);

    // Bundle entity declaration
    writeln!(turtle, "<{biri}> a bobbin:Bundle ;").unwrap();
    writeln!(turtle, "    rdfs:label \"{}\" ;", escape_turtle(&bundle.name)).unwrap();
    writeln!(
        turtle,
        "    bobbin:description \"{}\" ;",
        escape_turtle(&bundle.description)
    )
    .unwrap();
    writeln!(
        turtle,
        "    bobbin:slug \"{}\" .",
        escape_turtle(&bundle.slug())
    )
    .unwrap();
    writeln!(turtle).unwrap();

    // Keywords as individual triples
    for kw in &bundle.keywords {
        writeln!(
            turtle,
            "<{biri}> bobbin:keyword \"{}\" .",
            escape_turtle(kw)
        )
        .unwrap();
    }

    // Contains edges: files
    for f in &bundle.files {
        let miri = member_iri_for_file(repo_name, f);
        writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
    }

    // Contains edges: refs (CodeSymbol, Section, or CodeModule)
    for r in &bundle.refs {
        if let Some(parsed) = BundleRef::parse(r) {
            let miri = member_iri_from_ref(repo_name, &parsed);
            writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
        }
    }

    // Contains edges: docs
    for d in &bundle.docs {
        let miri = member_iri_for_doc(repo_name, d);
        writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
    }

    // Includes edges: sub-bundles
    for inc in &bundle.includes {
        let inc_iri = bundle_iri(inc);
        writeln!(turtle, "<{biri}> bobbin:includes <{inc_iri}> .").unwrap();
    }

    // Depends-on edges
    for dep in &bundle.depends_on {
        let dep_iri = bundle_iri(dep);
        writeln!(turtle, "<{biri}> bobbin:depends_on <{dep_iri}> .").unwrap();
    }

    // Bead references as literal properties
    for b in &bundle.beads {
        writeln!(
            turtle,
            "<{biri}> bobbin:bead_ref \"{}\" .",
            escape_turtle(b)
        )
        .unwrap();
    }

    // Tags as literal properties
    for t in &bundle.tags {
        writeln!(
            turtle,
            "<{biri}> bobbin:tag \"{}\" .",
            escape_turtle(t)
        )
        .unwrap();
    }

    turtle
}

/// Generate Turtle RDF for adding new members to an existing bundle.
#[allow(clippy::too_many_arguments)]
pub fn generate_add_turtle(
    bundle_name: &str,
    files: &[String],
    refs: &[String],
    docs: &[String],
    keywords: &[String],
    tags: &[String],
    includes: &[String],
    beads: &[String],
    repo_name: &str,
) -> String {
    let mut turtle = turtle_prefixes();
    let biri = bundle_iri(bundle_name);

    for f in files {
        let miri = member_iri_for_file(repo_name, f);
        writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
    }

    for r in refs {
        if let Some(parsed) = BundleRef::parse(r) {
            let miri = member_iri_from_ref(repo_name, &parsed);
            writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
        }
    }

    for d in docs {
        let miri = member_iri_for_doc(repo_name, d);
        writeln!(turtle, "<{biri}> bobbin:contains <{miri}> .").unwrap();
    }

    for kw in keywords {
        writeln!(
            turtle,
            "<{biri}> bobbin:keyword \"{}\" .",
            escape_turtle(kw)
        )
        .unwrap();
    }

    for t in tags {
        writeln!(
            turtle,
            "<{biri}> bobbin:tag \"{}\" .",
            escape_turtle(t)
        )
        .unwrap();
    }

    for inc in includes {
        let inc_iri = bundle_iri(inc);
        writeln!(turtle, "<{biri}> bobbin:includes <{inc_iri}> .").unwrap();
    }

    for b in beads {
        writeln!(
            turtle,
            "<{biri}> bobbin:bead_ref \"{}\" .",
            escape_turtle(b)
        )
        .unwrap();
    }

    turtle
}

/// Escape a string for Turtle literal values (double-quote delimited).
fn escape_turtle(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

/// Create a bundle in Quipu. Returns (tx_id, triple_count).
pub fn create_bundle(
    store: &mut quipu::Store,
    bundle: &BundleConfig,
    repo_name: &str,
) -> Result<(i64, usize)> {
    let turtle = generate_create_turtle(bundle, repo_name);
    ingest_turtle(store, &turtle, "bundle-create")
}

/// Add members to an existing bundle. Returns (tx_id, triple_count).
#[allow(clippy::too_many_arguments)]
pub fn add_members(
    store: &mut quipu::Store,
    bundle_name: &str,
    files: &[String],
    refs: &[String],
    docs: &[String],
    keywords: &[String],
    tags: &[String],
    includes: &[String],
    beads: &[String],
    repo_name: &str,
) -> Result<(i64, usize)> {
    let turtle = generate_add_turtle(
        bundle_name, files, refs, docs, keywords, tags, includes, beads, repo_name,
    );
    ingest_turtle(store, &turtle, "bundle-add")
}

/// Remove members from a bundle by retracting the bundle's `contains` predicate
/// and re-asserting the remaining edges. Returns (tx_id, retracted_count).
#[allow(clippy::too_many_arguments)]
pub fn remove_members(
    store: &mut quipu::Store,
    bundle_name: &str,
    remove_files: &[String],
    remove_refs: &[String],
    remove_docs: &[String],
    remove_keywords: &[String],
    remove_tags: &[String],
    remove_includes: &[String],
    remove_beads: &[String],
    repo_name: &str,
) -> Result<usize> {
    let biri = bundle_iri(bundle_name);

    // Build set of IRIs/values to remove
    let mut remove_contains_iris: Vec<String> = Vec::new();
    for f in remove_files {
        remove_contains_iris.push(member_iri_for_file(repo_name, f));
    }
    for r in remove_refs {
        if let Some(parsed) = BundleRef::parse(r) {
            remove_contains_iris.push(member_iri_from_ref(repo_name, &parsed));
        }
    }
    for d in remove_docs {
        remove_contains_iris.push(member_iri_for_doc(repo_name, d));
    }

    let remove_include_iris: Vec<String> = remove_includes.iter().map(|i| bundle_iri(i)).collect();

    // Query current members to know what to keep
    let detail = show_bundle(store, bundle_name)?;

    // Retract contains edges and re-assert kept ones
    let mut retracted = 0;
    if !remove_contains_iris.is_empty() {
        // Retract all contains edges
        let contains_iri = format!("{BOBBIN_NS}contains");
        let input = serde_json::json!({
            "entity": biri,
            "predicate": contains_iri,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin"
        });
        let result = quipu::tool_retract(store, &input)
            .map_err(|e| anyhow::anyhow!("retract contains edges: {e}"))?;
        retracted += result["retracted"].as_u64().unwrap_or(0) as usize;

        // Re-assert edges that should remain
        let mut turtle = turtle_prefixes();
        for member in &detail.members {
            if member.relationship == "contains"
                && !remove_contains_iris.contains(&member.iri)
            {
                writeln!(turtle, "<{biri}> bobbin:contains <{}> .", member.iri).unwrap();
            }
        }
        if turtle.lines().count() > 3 {
            // more than just prefixes
            ingest_turtle(store, &turtle, "bundle-remove-reassert")?;
        }
    }

    if !remove_include_iris.is_empty() {
        let includes_iri = format!("{BOBBIN_NS}includes");
        let input = serde_json::json!({
            "entity": biri,
            "predicate": includes_iri,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin"
        });
        let result = quipu::tool_retract(store, &input)
            .map_err(|e| anyhow::anyhow!("retract includes edges: {e}"))?;
        retracted += result["retracted"].as_u64().unwrap_or(0) as usize;

        // Re-assert kept includes
        let mut turtle = turtle_prefixes();
        for member in &detail.members {
            if member.relationship == "includes"
                && !remove_include_iris.contains(&member.iri)
            {
                writeln!(turtle, "<{biri}> bobbin:includes <{}> .", member.iri).unwrap();
            }
        }
        if turtle.lines().count() > 3 {
            ingest_turtle(store, &turtle, "bundle-remove-reassert")?;
        }
    }

    // Retract keyword literals
    if !remove_keywords.is_empty() {
        let keyword_iri = format!("{BOBBIN_NS}keyword");
        let input = serde_json::json!({
            "entity": biri,
            "predicate": keyword_iri,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin"
        });
        let result = quipu::tool_retract(store, &input)
            .map_err(|e| anyhow::anyhow!("retract keywords: {e}"))?;
        retracted += result["retracted"].as_u64().unwrap_or(0) as usize;

        // Re-assert kept keywords
        let mut turtle = turtle_prefixes();
        for kw in &detail.keywords {
            if !remove_keywords.contains(kw) {
                writeln!(
                    turtle,
                    "<{biri}> bobbin:keyword \"{}\" .",
                    escape_turtle(kw)
                )
                .unwrap();
            }
        }
        if turtle.lines().count() > 3 {
            ingest_turtle(store, &turtle, "bundle-remove-reassert")?;
        }
    }

    // Retract tag literals
    if !remove_tags.is_empty() {
        let tag_iri = format!("{BOBBIN_NS}tag");
        let input = serde_json::json!({
            "entity": biri,
            "predicate": tag_iri,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin"
        });
        let result = quipu::tool_retract(store, &input)
            .map_err(|e| anyhow::anyhow!("retract tags: {e}"))?;
        retracted += result["retracted"].as_u64().unwrap_or(0) as usize;

        // Re-assert kept tags
        let mut turtle = turtle_prefixes();
        for t in &detail.tags {
            if !remove_tags.contains(t) {
                writeln!(
                    turtle,
                    "<{biri}> bobbin:tag \"{}\" .",
                    escape_turtle(t)
                )
                .unwrap();
            }
        }
        if turtle.lines().count() > 3 {
            ingest_turtle(store, &turtle, "bundle-remove-reassert")?;
        }
    }

    // Retract bead literals
    if !remove_beads.is_empty() {
        let bead_iri = format!("{BOBBIN_NS}bead_ref");
        let input = serde_json::json!({
            "entity": biri,
            "predicate": bead_iri,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "actor": "bobbin"
        });
        let result = quipu::tool_retract(store, &input)
            .map_err(|e| anyhow::anyhow!("retract beads: {e}"))?;
        retracted += result["retracted"].as_u64().unwrap_or(0) as usize;

        // Re-assert kept beads
        let mut turtle = turtle_prefixes();
        for b in &detail.beads {
            if !remove_beads.contains(b) {
                writeln!(
                    turtle,
                    "<{biri}> bobbin:bead_ref \"{}\" .",
                    escape_turtle(b)
                )
                .unwrap();
            }
        }
        if turtle.lines().count() > 3 {
            ingest_turtle(store, &turtle, "bundle-remove-reassert")?;
        }
    }

    Ok(retracted)
}

/// Remove an entire bundle by retracting all its facts.
pub fn remove_bundle(store: &mut quipu::Store, bundle_name: &str) -> Result<usize> {
    let biri = bundle_iri(bundle_name);
    let input = serde_json::json!({
        "entity": biri,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "actor": "bobbin"
    });
    let result = quipu::tool_retract(store, &input)
        .map_err(|e| anyhow::anyhow!("retract bundle: {e}"))?;
    Ok(result["retracted"].as_u64().unwrap_or(0) as usize)
}

// ---------------------------------------------------------------------------
// Query operations
// ---------------------------------------------------------------------------

/// A bundle as retrieved from Quipu.
#[derive(Debug)]
pub struct QuipuBundle {
    pub name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    pub member_count: usize,
}

/// A member of a bundle in Quipu.
#[derive(Debug)]
pub struct QuipuBundleMember {
    pub iri: String,
    #[allow(dead_code)]
    pub label: Option<String>,
    pub relationship: String,
}

/// Full bundle detail from Quipu.
#[derive(Debug)]
pub struct QuipuBundleDetail {
    pub name: String,
    pub description: Option<String>,
    pub slug: Option<String>,
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
    pub beads: Vec<String>,
    pub members: Vec<QuipuBundleMember>,
}

/// List all bundles from Quipu.
pub fn list_bundles(store: &quipu::Store) -> Result<Vec<QuipuBundle>> {
    let sparql = format!(
        "PREFIX bobbin: <{BOBBIN_NS}>
         PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
         SELECT ?bundle ?label ?desc ?slug (COUNT(?member) AS ?count) WHERE {{
           ?bundle a bobbin:Bundle .
           ?bundle rdfs:label ?label .
           OPTIONAL {{ ?bundle bobbin:description ?desc }}
           OPTIONAL {{ ?bundle bobbin:slug ?slug }}
           OPTIONAL {{ ?bundle bobbin:contains ?member }}
         }} GROUP BY ?bundle ?label ?desc ?slug"
    );

    let input = serde_json::json!({ "query": sparql });
    let result = quipu::tool_query(store, &input)
        .map_err(|e| anyhow::anyhow!("SPARQL list bundles: {e}"))?;

    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    let mut bundles = Vec::new();
    for row in &rows {
        let label = row["label"].as_str().unwrap_or("").to_string();
        bundles.push(QuipuBundle {
            name: label.clone(),
            description: row["desc"].as_str().map(String::from),
            slug: row["slug"].as_str().map(String::from),
            member_count: row["count"]
                .as_u64()
                .or_else(|| row["count"].as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0) as usize,
        });
    }
    Ok(bundles)
}

/// Check whether a bundle exists in Quipu.
pub fn bundle_exists(store: &quipu::Store, name: &str) -> Result<bool> {
    let biri = bundle_iri(name);
    let sparql = format!(
        "PREFIX bobbin: <{BOBBIN_NS}>
         ASK {{ <{biri}> a bobbin:Bundle }}"
    );
    let input = serde_json::json!({ "query": sparql });
    let result = quipu::tool_query(store, &input)
        .map_err(|e| anyhow::anyhow!("SPARQL ask bundle exists: {e}"))?;
    Ok(result["result"].as_bool().unwrap_or(false))
}

/// Show bundle detail from Quipu.
pub fn show_bundle(store: &quipu::Store, name: &str) -> Result<QuipuBundleDetail> {
    let biri = bundle_iri(name);

    // Query bundle properties
    let props_sparql = format!(
        "PREFIX bobbin: <{BOBBIN_NS}>
         PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
         SELECT ?pred ?val WHERE {{
           <{biri}> ?pred ?val .
         }}"
    );
    let input = serde_json::json!({ "query": props_sparql });
    let result = quipu::tool_query(store, &input)
        .map_err(|e| anyhow::anyhow!("SPARQL show bundle: {e}"))?;

    let rows = result["rows"].as_array().cloned().unwrap_or_default();

    let mut detail = QuipuBundleDetail {
        name: name.to_string(),
        description: None,
        slug: None,
        keywords: Vec::new(),
        tags: Vec::new(),
        beads: Vec::new(),
        members: Vec::new(),
    };

    let desc_pred = format!("{BOBBIN_NS}description");
    let slug_pred = format!("{BOBBIN_NS}slug");
    let keyword_pred = format!("{BOBBIN_NS}keyword");
    let tag_pred = format!("{BOBBIN_NS}tag");
    let bead_pred = format!("{BOBBIN_NS}bead_ref");
    let contains_pred = format!("{BOBBIN_NS}contains");
    let includes_pred = format!("{BOBBIN_NS}includes");
    let depends_on_pred = format!("{BOBBIN_NS}depends_on");

    for row in &rows {
        let pred = row["pred"].as_str().unwrap_or("");
        let val = row["val"].as_str().unwrap_or("");

        if pred == desc_pred {
            detail.description = Some(val.to_string());
        } else if pred == slug_pred {
            detail.slug = Some(val.to_string());
        } else if pred == keyword_pred {
            detail.keywords.push(val.to_string());
        } else if pred == tag_pred {
            detail.tags.push(val.to_string());
        } else if pred == bead_pred {
            detail.beads.push(val.to_string());
        } else if pred == contains_pred {
            detail.members.push(QuipuBundleMember {
                iri: val.to_string(),
                label: None,
                relationship: "contains".to_string(),
            });
        } else if pred == includes_pred {
            detail.members.push(QuipuBundleMember {
                iri: val.to_string(),
                label: None,
                relationship: "includes".to_string(),
            });
        } else if pred == depends_on_pred {
            detail.members.push(QuipuBundleMember {
                iri: val.to_string(),
                label: None,
                relationship: "depends_on".to_string(),
            });
        }
    }

    Ok(detail)
}

/// Show bundle with deep traversal (2+ hops) including infra entities.
pub fn show_bundle_deep(
    store: &quipu::Store,
    name: &str,
) -> Result<(QuipuBundleDetail, Vec<DeepRelation>)> {
    let detail = show_bundle(store, name)?;

    // For each direct member, query their outgoing edges (1 more hop)
    let mut deep_relations = Vec::new();
    for member in &detail.members {
        let sparql = format!(
            "PREFIX bobbin: <{BOBBIN_NS}>
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
             SELECT ?pred ?target ?label ?type WHERE {{
               <{}> ?pred ?target .
               OPTIONAL {{ ?target rdfs:label ?label }}
               OPTIONAL {{ ?target a ?type }}
               FILTER(?pred != <http://www.w3.org/1999/02/22-rdf-syntax-ns#type>)
             }}",
            member.iri
        );
        let input = serde_json::json!({ "query": sparql });
        if let Ok(result) = quipu::tool_query(store, &input) {
            let rows = result["rows"].as_array().cloned().unwrap_or_default();
            for row in &rows {
                deep_relations.push(DeepRelation {
                    source_iri: member.iri.clone(),
                    predicate: row["pred"].as_str().unwrap_or("").to_string(),
                    target_iri: row["target"].as_str().unwrap_or("").to_string(),
                    target_label: row["label"].as_str().map(String::from),
                    target_type: row["type"].as_str().map(String::from),
                });
            }
        }
    }

    Ok((detail, deep_relations))
}

/// A relation discovered via deep traversal.
#[derive(Debug)]
pub struct DeepRelation {
    pub source_iri: String,
    pub predicate: String,
    pub target_iri: String,
    pub target_label: Option<String>,
    pub target_type: Option<String>,
}

/// Check that bundle member IRIs resolve to actual files/symbols in the repo.
pub fn check_bundle(
    store: &quipu::Store,
    name: &str,
    repo_root: &Path,
    repo_name: &str,
) -> Result<Vec<CheckIssue>> {
    let detail = show_bundle(store, name)?;
    let mut issues = Vec::new();
    let code_prefix = format!("{BOBBIN_NS}code/{repo_name}/");
    let doc_prefix = format!("{BOBBIN_NS}doc/{repo_name}/");

    for member in &detail.members {
        if member.relationship != "contains" {
            continue;
        }

        if let Some(path_part) = member.iri.strip_prefix(&code_prefix) {
            // CodeModule or CodeSymbol
            let (file_path, symbol) = if let Some((fp, sym)) = path_part.split_once("::") {
                (fp, Some(sym))
            } else {
                (path_part, None)
            };

            let full_path = repo_root.join(file_path);
            if !full_path.exists() {
                issues.push(CheckIssue {
                    iri: member.iri.clone(),
                    issue: format!("file missing: {file_path}"),
                    severity: IssueSeverity::Error,
                });
            } else if let Some(sym) = symbol {
                // Check symbol exists in file
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    if !content.contains(sym) {
                        issues.push(CheckIssue {
                            iri: member.iri.clone(),
                            issue: format!("symbol not found: {sym} (in {file_path})"),
                            severity: IssueSeverity::Warning,
                        });
                    }
                }
            }
        } else if let Some(path_part) = member.iri.strip_prefix(&doc_prefix) {
            let (file_path, heading) = if let Some((fp, h)) = path_part.split_once('#') {
                (fp, Some(h))
            } else {
                (path_part, None)
            };

            let full_path = repo_root.join(file_path);
            if !full_path.exists() {
                issues.push(CheckIssue {
                    iri: member.iri.clone(),
                    issue: format!("doc missing: {file_path}"),
                    severity: IssueSeverity::Error,
                });
            } else if let Some(heading) = heading {
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let found = content
                        .lines()
                        .any(|l| l.trim_start_matches('#').trim().starts_with(heading));
                    if !found {
                        issues.push(CheckIssue {
                            iri: member.iri.clone(),
                            issue: format!("heading not found: {heading} (in {file_path})"),
                            severity: IssueSeverity::Warning,
                        });
                    }
                }
            }
        }
        // Bundle members (includes/depends_on) are validated via bundle_exists
    }

    // Check included bundles exist
    for member in &detail.members {
        if member.relationship == "includes" || member.relationship == "depends_on" {
            let bundle_name = member
                .iri
                .strip_prefix(&format!("{BOBBIN_NS}bundle/"))
                .unwrap_or(&member.iri);
            if !bundle_exists(store, bundle_name)? {
                issues.push(CheckIssue {
                    iri: member.iri.clone(),
                    issue: format!("{} bundle not found: {bundle_name}", member.relationship),
                    severity: IssueSeverity::Error,
                });
            }
        }
    }

    Ok(issues)
}

/// An issue found during bundle validation.
#[derive(Debug)]
pub struct CheckIssue {
    #[allow(dead_code)]
    pub iri: String,
    pub issue: String,
    pub severity: IssueSeverity,
}

#[derive(Debug)]
pub enum IssueSeverity {
    Error,
    Warning,
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Extract a human-readable label from a member IRI.
pub fn display_member_iri(iri: &str) -> String {
    if let Some(rest) = iri.strip_prefix(BOBBIN_NS) {
        rest.to_string()
    } else {
        iri.to_string()
    }
}

/// Extract the file path from a code/doc IRI, stripping the repo prefix.
pub fn file_path_from_iri(iri: &str, repo_name: &str) -> Option<String> {
    let code_prefix = format!("{BOBBIN_NS}code/{repo_name}/");
    let doc_prefix = format!("{BOBBIN_NS}doc/{repo_name}/");

    if let Some(rest) = iri.strip_prefix(&code_prefix) {
        // Strip symbol part if present
        let path = rest.split_once("::").map_or(rest, |(p, _)| p);
        Some(path.to_string())
    } else if let Some(rest) = iri.strip_prefix(&doc_prefix) {
        let path = rest.split_once('#').map_or(rest, |(p, _)| p);
        Some(path.to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Ingest Turtle RDF into the store via tool_knot.
fn ingest_turtle(store: &mut quipu::Store, turtle: &str, source: &str) -> Result<(i64, usize)> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let input = serde_json::json!({
        "turtle": turtle,
        "timestamp": timestamp,
        "actor": "bobbin",
        "source": source
    });

    let result = quipu::tool_knot(store, &input)
        .map_err(|e| anyhow::anyhow!("Failed to ingest turtle ({source}): {e}"))?;

    let tx_id = result["tx_id"].as_i64().unwrap_or(-1);
    let count = result["count"].as_u64().unwrap_or(0) as usize;
    Ok((tx_id, count))
}

/// Detect the repo name from the current directory (uses git remote or dir name).
pub fn detect_repo_name(repo_root: &Path) -> String {
    // Try git remote
    if let Ok(output) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Extract repo name from URL: "https://github.com/user/repo.git" → "repo"
            if let Some(name) = url.rsplit('/').next() {
                return name.trim_end_matches(".git").to_string();
            }
        }
    }
    // Fallback: directory name
    repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_iri() {
        assert_eq!(
            bundle_iri("context/pipeline"),
            "https://bobbin.dev/bundle/context/pipeline"
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
    fn test_document_iri() {
        assert_eq!(
            document_iri("bobbin", "docs/design.md"),
            "https://bobbin.dev/doc/bobbin/docs/design.md"
        );
    }

    #[test]
    fn test_section_iri() {
        assert_eq!(
            section_iri("bobbin", "docs/design.md", "Overview"),
            "https://bobbin.dev/doc/bobbin/docs/design.md#Overview"
        );
    }

    #[test]
    fn test_escape_turtle() {
        assert_eq!(escape_turtle("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(escape_turtle("line\nnewline"), "line\\nnewline");
    }

    #[test]
    fn test_generate_create_turtle_basic() {
        let bundle = BundleConfig {
            name: "search/hybrid".to_string(),
            description: "Hybrid search pipeline".to_string(),
            keywords: vec!["search".to_string(), "hybrid".to_string()],
            tags: vec![],
            files: vec!["src/search/hybrid.rs".to_string()],
            refs: vec!["src/search/context.rs::ContextAssembler".to_string()],
            docs: vec!["docs/design.md".to_string()],
            beads: vec![],
            includes: vec!["search/reranking".to_string()],
            implements: vec![],
            depends_on: vec![],
            tests: vec![],
            repos: vec![],
            slug: None,
        };

        let turtle = generate_create_turtle(&bundle, "bobbin");

        // Check entity declaration
        assert!(turtle.contains("a bobbin:Bundle"));
        assert!(turtle.contains("rdfs:label \"search/hybrid\""));
        assert!(turtle.contains("bobbin:description \"Hybrid search pipeline\""));
        assert!(turtle.contains("bobbin:slug \"search-hybrid\""));

        // Check keywords
        assert!(turtle.contains("bobbin:keyword \"search\""));
        assert!(turtle.contains("bobbin:keyword \"hybrid\""));

        // Check contains edges
        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/code/bobbin/src/search/hybrid.rs>"));
        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/code/bobbin/src/search/context.rs::ContextAssembler>"));
        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/doc/bobbin/docs/design.md>"));

        // Check includes edge
        assert!(turtle.contains("bobbin:includes <https://bobbin.dev/bundle/search/reranking>"));
    }

    #[test]
    fn test_generate_add_turtle() {
        let turtle = generate_add_turtle(
            "search/hybrid",
            &["src/new_file.rs".to_string()],
            &[],
            &[],
            &["ranking".to_string()],
            &[],
            &[],
            &[],
            "bobbin",
        );

        assert!(turtle.contains("bobbin:contains <https://bobbin.dev/code/bobbin/src/new_file.rs>"));
        assert!(turtle.contains("bobbin:keyword \"ranking\""));
    }

    #[test]
    fn test_display_member_iri() {
        assert_eq!(
            display_member_iri("https://bobbin.dev/code/bobbin/src/main.rs"),
            "code/bobbin/src/main.rs"
        );
        assert_eq!(
            display_member_iri("https://bobbin.dev/bundle/search/hybrid"),
            "bundle/search/hybrid"
        );
    }

    #[test]
    fn test_file_path_from_iri() {
        assert_eq!(
            file_path_from_iri("https://bobbin.dev/code/bobbin/src/main.rs", "bobbin"),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            file_path_from_iri(
                "https://bobbin.dev/code/bobbin/src/tags.rs::BundleConfig",
                "bobbin"
            ),
            Some("src/tags.rs".to_string())
        );
        assert_eq!(
            file_path_from_iri("https://bobbin.dev/doc/bobbin/docs/d.md#Heading", "bobbin"),
            Some("docs/d.md".to_string())
        );
        assert_eq!(
            file_path_from_iri("https://other.dev/unrelated", "bobbin"),
            None
        );
    }

    #[test]
    fn test_member_iri_from_ref_whole_file() {
        let r = BundleRef::parse("src/main.rs").unwrap();
        assert_eq!(
            member_iri_from_ref("bobbin", &r),
            "https://bobbin.dev/code/bobbin/src/main.rs"
        );
    }

    #[test]
    fn test_member_iri_from_ref_symbol() {
        let r = BundleRef::parse("src/tags.rs::BundleConfig").unwrap();
        assert_eq!(
            member_iri_from_ref("bobbin", &r),
            "https://bobbin.dev/code/bobbin/src/tags.rs::BundleConfig"
        );
    }

    #[test]
    fn test_member_iri_from_ref_heading() {
        let r = BundleRef::parse("docs/design.md#Overview").unwrap();
        assert_eq!(
            member_iri_from_ref("bobbin", &r),
            "https://bobbin.dev/doc/bobbin/docs/design.md#Overview"
        );
    }

    #[test]
    fn test_member_iri_from_ref_cross_repo() {
        let r = BundleRef::parse("aegis:src/deploy.rs::deploy").unwrap();
        assert_eq!(
            member_iri_from_ref("bobbin", &r),
            "https://bobbin.dev/code/aegis/src/deploy.rs::deploy"
        );
    }
}
