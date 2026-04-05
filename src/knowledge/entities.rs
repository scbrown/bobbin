//! Sync tree-sitter AST entities to Quipu knowledge graph.
//!
//! After `bobbin index` completes, this module converts code chunks to
//! CodeModule and CodeSymbol entities with structural relationships
//! (defines, imports, contains, implements) and pushes them to Quipu.
//!
//! The sync is incremental: only diffs are pushed, tracked via
//! `.bobbin/knowledge-sync.json`.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::{ChunkEdge, ChunkEdgeType, ChunkType, ImportEdge};

const BOBBIN_NS: &str = "https://bobbin.dev/";

/// SHACL shapes for code entity validation.
const CODE_ENTITY_SHAPES: &str = r#"@prefix bobbin: <https://bobbin.dev/> .
@prefix sh:     <http://www.w3.org/ns/shacl#> .
@prefix xsd:    <http://www.w3.org/2001/XMLSchema#> .
@prefix rdf:    <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

bobbin:CodeModuleShape a sh:NodeShape ;
    sh:targetClass bobbin:CodeModule ;
    sh:property [
        sh:path bobbin:filePath ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:repo ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:language ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] .

bobbin:CodeSymbolShape a sh:NodeShape ;
    sh:targetClass bobbin:CodeSymbol ;
    sh:property [
        sh:path bobbin:name ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:symbolKind ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:datatype xsd:string ;
    ] ;
    sh:property [
        sh:path bobbin:definedIn ;
        sh:minCount 1 ; sh:maxCount 1 ;
        sh:class bobbin:CodeModule ;
    ] .
"#;

/// Lightweight entity info collected during indexing (before chunks are consumed).
#[derive(Debug, Clone)]
pub struct FileEntityInfo {
    /// Relative file path (e.g., "src/main.rs")
    pub rel_path: String,
    /// Detected language (e.g., "rust", "python")
    pub language: String,
    /// Symbols extracted from this file's chunks
    pub symbols: Vec<SymbolEntityInfo>,
}

/// A code symbol extracted from a tree-sitter chunk.
#[derive(Debug, Clone)]
pub struct SymbolEntityInfo {
    /// Symbol name (e.g., "main", "Parser", "ChunkType")
    pub name: String,
    /// Symbol kind mapped from ChunkType (e.g., "function", "struct", "trait")
    pub kind: String,
}

/// Persisted sync state for incremental entity sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncState {
    /// Quipu transaction ID of the last successful sync.
    last_sync_tx: i64,
    /// Map of entity IRI → content hash for change detection.
    entities: HashMap<String, String>,
}

impl SyncState {
    fn load(repo_root: &Path) -> Self {
        let path = sync_state_path(repo_root);
        if let Ok(content) = std::fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self, repo_root: &Path) -> Result<()> {
        let path = sync_state_path(repo_root);
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write sync state to {}", path.display()))?;
        Ok(())
    }
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            last_sync_tx: -1,
            entities: HashMap::new(),
        }
    }
}

fn sync_state_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(".bobbin").join("knowledge-sync.json")
}

/// Collect entity info from parsed chunks for a single file.
///
/// Call this during the indexing loop before chunks are consumed by embedding.
pub fn collect_file_entities(
    rel_path: &str,
    language: &str,
    chunks: &[crate::types::Chunk],
) -> FileEntityInfo {
    let symbols: Vec<SymbolEntityInfo> = chunks
        .iter()
        .filter_map(|chunk| {
            let kind = chunk_type_to_symbol_kind(chunk.chunk_type)?;
            let name = chunk.name.as_ref()?;
            Some(SymbolEntityInfo {
                name: name.clone(),
                kind: kind.to_string(),
            })
        })
        .collect();

    FileEntityInfo {
        rel_path: rel_path.to_string(),
        language: language.to_string(),
        symbols,
    }
}

/// Push code entities to the Quipu knowledge graph.
///
/// Performs incremental sync: compares current entities against the previous
/// sync state, pushes only diffs, and tracks removed entities.
///
/// Returns `(transaction_id, entity_count)` on success.
pub fn push_entities_to_quipu(
    file_entities: &[FileEntityInfo],
    imports: &[ImportEdge],
    chunk_edges: &[ChunkEdge],
    repo_name: &str,
    repo_root: &Path,
) -> Result<(i64, usize)> {
    if file_entities.is_empty() {
        return Ok((-1, 0));
    }

    // Build current entity set with content hashes
    let mut current_entities: HashMap<String, String> = HashMap::new();

    for file in file_entities {
        // CodeModule entity
        let module_iri = code_module_iri(repo_name, &file.rel_path);
        let module_hash = hash_entity(&[
            &module_iri,
            &file.rel_path,
            repo_name,
            &file.language,
        ]);
        current_entities.insert(module_iri, module_hash);

        // CodeSymbol entities
        for symbol in &file.symbols {
            let symbol_iri = code_symbol_iri(repo_name, &file.rel_path, &symbol.name);
            let symbol_hash = hash_entity(&[
                &symbol_iri,
                &symbol.name,
                &symbol.kind,
                &file.rel_path,
            ]);
            current_entities.insert(symbol_iri, symbol_hash);
        }
    }

    // Load previous sync state
    let prev_state = SyncState::load(repo_root);

    // Compute diff
    let added: Vec<&String> = current_entities
        .keys()
        .filter(|iri| !prev_state.entities.contains_key(*iri))
        .collect();
    let changed: Vec<&String> = current_entities
        .keys()
        .filter(|iri| {
            prev_state
                .entities
                .get(*iri)
                .map_or(false, |old_hash| old_hash != &current_entities[*iri])
        })
        .collect();
    let removed: Vec<&String> = prev_state
        .entities
        .keys()
        .filter(|iri| !current_entities.contains_key(*iri))
        .collect();

    let to_sync: Vec<&String> = added.iter().chain(changed.iter()).copied().collect();

    if to_sync.is_empty() && removed.is_empty() {
        // Nothing changed — update sync state to reflect current set
        let new_state = SyncState {
            last_sync_tx: prev_state.last_sync_tx,
            entities: current_entities,
        };
        new_state.save(repo_root)?;
        return Ok((prev_state.last_sync_tx, 0));
    }

    // Build set of IRIs that need syncing for filtering
    let sync_iris: std::collections::HashSet<&String> = to_sync.iter().copied().collect();

    // Generate Turtle RDF for new/changed entities
    let turtle = generate_entity_turtle(
        file_entities,
        imports,
        chunk_edges,
        repo_name,
        &sync_iris,
    );

    if turtle.trim().lines().count() <= 3 {
        // Only prefix declarations, no actual entities to push
        let new_state = SyncState {
            last_sync_tx: prev_state.last_sync_tx,
            entities: current_entities,
        };
        new_state.save(repo_root)?;
        return Ok((prev_state.last_sync_tx, 0));
    }

    // Open Quipu store
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

    // Push entities via tool_knot with SHACL validation
    let timestamp = chrono::Utc::now().to_rfc3339();
    let input = serde_json::json!({
        "turtle": turtle,
        "timestamp": timestamp,
        "actor": "bobbin",
        "source": "entity-sync",
        "shapes": CODE_ENTITY_SHAPES,
    });

    let result = quipu::tool_knot(&mut store, &input)
        .map_err(|e| anyhow::anyhow!("Failed to push entities to quipu: {e}"))?;

    let tx_id = result["tx_id"].as_i64().unwrap_or(-1);
    let count = result["count"].as_u64().unwrap_or(0) as usize;

    // Handle retraction of removed entities
    if !removed.is_empty() {
        let retract_turtle = generate_retraction_turtle(&removed, repo_name);
        if !retract_turtle.is_empty() {
            let retract_input = serde_json::json!({
                "turtle": retract_turtle,
                "timestamp": timestamp,
                "actor": "bobbin",
                "source": "entity-sync-retract",
            });

            // Best-effort retraction: log but don't fail if retraction fails
            if let Err(e) = quipu::tool_knot(&mut store, &retract_input) {
                tracing::warn!("Failed to retract {} removed entities: {}", removed.len(), e);
            }
        }
    }

    // Save updated sync state
    let new_state = SyncState {
        last_sync_tx: tx_id,
        entities: current_entities,
    };
    new_state.save(repo_root)?;

    Ok((tx_id, count))
}

/// Generate Turtle RDF for code entities and their relationships.
///
/// Only includes entities whose IRIs are in the `sync_iris` set.
/// Relationships involving at least one synced entity are included.
fn generate_entity_turtle(
    file_entities: &[FileEntityInfo],
    imports: &[ImportEdge],
    chunk_edges: &[ChunkEdge],
    repo_name: &str,
    sync_iris: &std::collections::HashSet<&String>,
) -> String {
    let mut turtle = String::with_capacity(file_entities.len() * 1024);

    // Prefixes
    writeln!(turtle, "@prefix bobbin: <{BOBBIN_NS}> .").unwrap();
    writeln!(turtle, "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .").unwrap();
    writeln!(turtle, "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .").unwrap();
    writeln!(turtle).unwrap();

    for file in file_entities {
        let module_iri = code_module_iri(repo_name, &file.rel_path);
        let module_needs_sync = sync_iris.contains(&module_iri);

        // CodeModule entity
        if module_needs_sync {
            writeln!(turtle, "<{module_iri}> a bobbin:CodeModule ;").unwrap();
            writeln!(
                turtle,
                "    bobbin:filePath \"{}\"^^xsd:string ;",
                escape_turtle_string(&file.rel_path)
            )
            .unwrap();
            writeln!(
                turtle,
                "    bobbin:repo \"{}\"^^xsd:string ;",
                escape_turtle_string(repo_name)
            )
            .unwrap();
            writeln!(
                turtle,
                "    bobbin:language \"{}\"^^xsd:string .",
                escape_turtle_string(&file.language)
            )
            .unwrap();
            writeln!(turtle).unwrap();
        }

        // CodeSymbol entities + defines edges
        for symbol in &file.symbols {
            let symbol_iri = code_symbol_iri(repo_name, &file.rel_path, &symbol.name);
            let symbol_needs_sync = sync_iris.contains(&symbol_iri);

            if symbol_needs_sync {
                writeln!(turtle, "<{symbol_iri}> a bobbin:CodeSymbol ;").unwrap();
                writeln!(
                    turtle,
                    "    bobbin:name \"{}\"^^xsd:string ;",
                    escape_turtle_string(&symbol.name)
                )
                .unwrap();
                writeln!(
                    turtle,
                    "    bobbin:symbolKind \"{}\"^^xsd:string ;",
                    escape_turtle_string(&symbol.kind)
                )
                .unwrap();
                writeln!(turtle, "    bobbin:definedIn <{module_iri}> .").unwrap();
                writeln!(turtle).unwrap();
            }

            // defines edge (module → symbol)
            if module_needs_sync || symbol_needs_sync {
                writeln!(
                    turtle,
                    "<{module_iri}> bobbin:defines <{symbol_iri}> ."
                )
                .unwrap();
            }
        }
    }

    // Import edges: CodeModule → unresolved import specifier or resolved CodeModule
    for imp in imports {
        let source_iri = code_module_iri(repo_name, &imp.source_file);
        if !sync_iris.contains(&source_iri) {
            continue;
        }

        if let Some(ref resolved) = imp.resolved_path {
            let target_iri = code_module_iri(repo_name, resolved);
            writeln!(
                turtle,
                "<{source_iri}> bobbin:imports <{target_iri}> ."
            )
            .unwrap();
        } else {
            // Unresolved import: store as a literal string
            writeln!(
                turtle,
                "<{source_iri}> bobbin:importsUnresolved \"{}\"^^xsd:string .",
                escape_turtle_string(&imp.import_specifier)
            )
            .unwrap();
        }
    }

    // Chunk-level structural edges
    for edge in chunk_edges {
        let source_name = &edge.source_name;
        let target_name = &edge.target_name;
        let file_path = &edge.file_path;

        let source_iri = code_symbol_iri(repo_name, file_path, source_name);
        let target_iri = code_symbol_iri(repo_name, file_path, target_name);

        // Only emit if at least one side is being synced
        if !sync_iris.contains(&source_iri) && !sync_iris.contains(&target_iri) {
            continue;
        }

        let predicate = match edge.edge_type {
            ChunkEdgeType::Implements => "bobbin:implements",
            ChunkEdgeType::ImplFor => "bobbin:contains",
            ChunkEdgeType::Extends => "bobbin:extends",
            ChunkEdgeType::Tests => "bobbin:tests",
        };

        writeln!(turtle, "<{source_iri}> {predicate} <{target_iri}> .").unwrap();
    }

    turtle
}

/// Generate Turtle RDF to mark removed entities as retracted.
///
/// Asserts `bobbin:retractedAt` with the current timestamp so that
/// temporal queries can filter these out.
fn generate_retraction_turtle(removed_iris: &[&String], _repo_name: &str) -> String {
    if removed_iris.is_empty() {
        return String::new();
    }

    let mut turtle = String::with_capacity(removed_iris.len() * 128);
    writeln!(turtle, "@prefix bobbin: <{BOBBIN_NS}> .").unwrap();
    writeln!(turtle, "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .").unwrap();
    writeln!(turtle).unwrap();

    let now = chrono::Utc::now().to_rfc3339();
    for iri in removed_iris {
        writeln!(
            turtle,
            "<{iri}> bobbin:retractedAt \"{}\"^^xsd:dateTime .",
            now
        )
        .unwrap();
    }

    turtle
}

/// Map ChunkType to a symbol kind string for Quipu entities.
fn chunk_type_to_symbol_kind(chunk_type: ChunkType) -> Option<&'static str> {
    match chunk_type {
        ChunkType::Function => Some("function"),
        ChunkType::Method => Some("method"),
        ChunkType::Struct => Some("struct"),
        ChunkType::Enum => Some("enum"),
        ChunkType::Trait => Some("trait"),
        ChunkType::Impl => Some("impl"),
        ChunkType::Class => Some("class"),
        ChunkType::Interface => Some("interface"),
        ChunkType::Module => Some("module"),
        // Doc, Section, Table, CodeBlock, Commit, Issue, Other are not code symbols
        _ => None,
    }
}

/// Build the IRI for a CodeModule entity.
fn code_module_iri(repo: &str, path: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}",
        iri_encode(repo),
        iri_encode(path)
    )
}

/// Build the IRI for a CodeSymbol entity.
fn code_symbol_iri(repo: &str, path: &str, symbol: &str) -> String {
    format!(
        "{BOBBIN_NS}code/{}/{}::{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(symbol)
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

/// Escape a string for use in Turtle RDF literal.
fn escape_turtle_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Compute a stable hash from a set of property values.
fn hash_entity(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Chunk;

    #[test]
    fn test_collect_file_entities() {
        let chunks = vec![
            Chunk {
                id: "src/main.rs:1:10".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("main".to_string()),
                start_line: 1,
                end_line: 10,
                content: "fn main() {}".to_string(),
                language: "rust".to_string(),
                tags: String::new(),
            },
            Chunk {
                id: "src/main.rs:12:30".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Struct,
                name: Some("Config".to_string()),
                start_line: 12,
                end_line: 30,
                content: "struct Config {}".to_string(),
                language: "rust".to_string(),
                tags: String::new(),
            },
            Chunk {
                id: "src/main.rs:32:35".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Doc,
                name: Some("README".to_string()),
                start_line: 32,
                end_line: 35,
                content: "# README".to_string(),
                language: "markdown".to_string(),
                tags: String::new(),
            },
        ];

        let info = collect_file_entities("src/main.rs", "rust", &chunks);
        assert_eq!(info.rel_path, "src/main.rs");
        assert_eq!(info.language, "rust");
        assert_eq!(info.symbols.len(), 2);
        assert_eq!(info.symbols[0].name, "main");
        assert_eq!(info.symbols[0].kind, "function");
        assert_eq!(info.symbols[1].name, "Config");
        assert_eq!(info.symbols[1].kind, "struct");
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
            code_symbol_iri("bobbin", "src/main.rs", "main"),
            "https://bobbin.dev/code/bobbin/src/main.rs::main"
        );
    }

    #[test]
    fn test_escape_turtle_string() {
        assert_eq!(escape_turtle_string("hello"), "hello");
        assert_eq!(escape_turtle_string("he\"llo"), "he\\\"llo");
        assert_eq!(escape_turtle_string("line\nnew"), "line\\nnew");
    }

    #[test]
    fn test_chunk_type_to_symbol_kind() {
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Function), Some("function"));
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Struct), Some("struct"));
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Trait), Some("trait"));
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Impl), Some("impl"));
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Doc), None);
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Section), None);
        assert_eq!(chunk_type_to_symbol_kind(ChunkType::Other), None);
    }

    #[test]
    fn test_generate_entity_turtle_basic() {
        let files = vec![FileEntityInfo {
            rel_path: "src/main.rs".to_string(),
            language: "rust".to_string(),
            symbols: vec![
                SymbolEntityInfo {
                    name: "main".to_string(),
                    kind: "function".to_string(),
                },
                SymbolEntityInfo {
                    name: "Config".to_string(),
                    kind: "struct".to_string(),
                },
            ],
        }];

        // All entities are new → all IRIs in sync set
        let all_iris: Vec<String> = vec![
            code_module_iri("myrepo", "src/main.rs"),
            code_symbol_iri("myrepo", "src/main.rs", "main"),
            code_symbol_iri("myrepo", "src/main.rs", "Config"),
        ];
        let sync_iris: std::collections::HashSet<&String> = all_iris.iter().collect();

        let turtle = generate_entity_turtle(&files, &[], &[], "myrepo", &sync_iris);

        assert!(turtle.contains("a bobbin:CodeModule"));
        assert!(turtle.contains("bobbin:filePath \"src/main.rs\""));
        assert!(turtle.contains("bobbin:repo \"myrepo\""));
        assert!(turtle.contains("bobbin:language \"rust\""));
        assert!(turtle.contains("a bobbin:CodeSymbol"));
        assert!(turtle.contains("bobbin:name \"main\""));
        assert!(turtle.contains("bobbin:symbolKind \"function\""));
        assert!(turtle.contains("bobbin:defines"));
        assert!(turtle.contains("bobbin:definedIn"));
    }

    #[test]
    fn test_generate_entity_turtle_with_imports() {
        let files = vec![FileEntityInfo {
            rel_path: "src/main.rs".to_string(),
            language: "rust".to_string(),
            symbols: vec![],
        }];

        let imports = vec![
            ImportEdge {
                source_file: "src/main.rs".to_string(),
                import_specifier: "crate::config".to_string(),
                resolved_path: Some("src/config.rs".to_string()),
                language: "rust".to_string(),
            },
            ImportEdge {
                source_file: "src/main.rs".to_string(),
                import_specifier: "serde::Serialize".to_string(),
                resolved_path: None,
                language: "rust".to_string(),
            },
        ];

        let module_iri = code_module_iri("repo", "src/main.rs");
        let sync_iris: std::collections::HashSet<&String> =
            [&module_iri].into_iter().collect();

        let turtle = generate_entity_turtle(&files, &imports, &[], "repo", &sync_iris);

        assert!(turtle.contains("bobbin:imports"));
        assert!(turtle.contains("bobbin:importsUnresolved \"serde::Serialize\""));
    }

    #[test]
    fn test_generate_retraction_turtle() {
        let iris = vec![
            "https://bobbin.dev/code/repo/deleted.rs".to_string(),
        ];
        let refs: Vec<&String> = iris.iter().collect();

        let turtle = generate_retraction_turtle(&refs, "repo");
        assert!(turtle.contains("bobbin:retractedAt"));
        assert!(turtle.contains("deleted.rs"));
    }

    #[test]
    fn test_hash_entity_deterministic() {
        let h1 = hash_entity(&["a", "b", "c"]);
        let h2 = hash_entity(&["a", "b", "c"]);
        let h3 = hash_entity(&["a", "b", "d"]);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_sync_state_default() {
        let state = SyncState::default();
        assert_eq!(state.last_sync_tx, -1);
        assert!(state.entities.is_empty());
    }

    #[test]
    fn test_sync_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let bobbin_dir = dir.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();

        let mut state = SyncState::default();
        state.last_sync_tx = 42;
        state
            .entities
            .insert("https://bobbin.dev/code/repo/a.rs".to_string(), "hash1".to_string());

        state.save(dir.path()).unwrap();
        let loaded = SyncState::load(dir.path());
        assert_eq!(loaded.last_sync_tx, 42);
        assert_eq!(
            loaded.entities.get("https://bobbin.dev/code/repo/a.rs"),
            Some(&"hash1".to_string())
        );
    }

    #[test]
    fn test_generate_entity_turtle_with_chunk_edges() {
        let files = vec![FileEntityInfo {
            rel_path: "src/parser.rs".to_string(),
            language: "rust".to_string(),
            symbols: vec![
                SymbolEntityInfo {
                    name: "Parser".to_string(),
                    kind: "struct".to_string(),
                },
                SymbolEntityInfo {
                    name: "impl Parser".to_string(),
                    kind: "impl".to_string(),
                },
            ],
        }];

        let chunk_edges = vec![ChunkEdge {
            source_chunk: "src/parser.rs:10:50".to_string(),
            target_chunk: "src/parser.rs:1:8".to_string(),
            source_name: "impl Parser".to_string(),
            target_name: "Parser".to_string(),
            edge_type: ChunkEdgeType::ImplFor,
            file_path: "src/parser.rs".to_string(),
        }];

        let all_iris: Vec<String> = vec![
            code_module_iri("repo", "src/parser.rs"),
            code_symbol_iri("repo", "src/parser.rs", "Parser"),
            code_symbol_iri("repo", "src/parser.rs", "impl Parser"),
        ];
        let sync_iris: std::collections::HashSet<&String> = all_iris.iter().collect();

        let turtle =
            generate_entity_turtle(&files, &[], &chunk_edges, "repo", &sync_iris);

        assert!(turtle.contains("bobbin:contains"));
    }
}
