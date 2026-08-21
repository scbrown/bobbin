//! `Tests` edges inferred from naming conventions, language-agnostically.
//!
//! `ChunkEdgeType::Tests` was declared with the other edge types but no
//! collector ever emitted it. This pass links a test function to the
//! production symbol its name points at, within one file's chunks:
//! `test_foo` / `foo_test` → `foo`, `TestFoo` / `testFoo` → `Foo`.
//! Cross-file test linkage (tests/ directories) stays with the co-change
//! coupling signal — a name alone is not evidence across files.

use crate::types::{Chunk, ChunkEdge, ChunkEdgeType, ChunkType};

/// Candidate production-symbol names implied by a test's name.
fn tested_names(test_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rest) = test_name.strip_prefix("test_") {
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    } else if let Some(rest) = test_name.strip_suffix("_test") {
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    } else if let Some(rest) = test_name.strip_prefix("Test").or_else(|| {
        test_name
            .strip_prefix("test")
            .filter(|r| r.starts_with(char::is_uppercase))
    }) {
        if !rest.is_empty() {
            out.push(rest.to_string());
            let mut lower = rest.to_string();
            lower.replace_range(..1, &rest[..1].to_lowercase());
            if lower != rest {
                out.push(lower);
            }
        }
    }
    out
}

/// Emit `Tests` edges for one file's chunks.
///
/// Only Function/Method chunks participate on either end, a chunk never
/// tests itself, and an unresolvable name degrades to no edge.
pub fn extract_test_edges(file_path: &str, chunks: &[Chunk]) -> Vec<ChunkEdge> {
    let callable = |c: &&Chunk| matches!(c.chunk_type, ChunkType::Function | ChunkType::Method);
    let by_name: std::collections::HashMap<&str, &Chunk> = chunks
        .iter()
        .filter(|c| callable(c) || matches!(c.chunk_type, ChunkType::Struct | ChunkType::Class))
        .filter_map(|c| c.name.as_deref().map(|n| (n, c)))
        .collect();

    let mut edges = Vec::new();
    for chunk in chunks.iter().filter(callable) {
        let Some(name) = chunk.name.as_deref() else {
            continue;
        };
        for candidate in tested_names(name) {
            if let Some(target) = by_name.get(candidate.as_str()) {
                if target.id != chunk.id {
                    edges.push(ChunkEdge {
                        source_chunk: chunk.id.clone(),
                        target_chunk: target.id.clone(),
                        source_name: name.to_string(),
                        target_name: candidate,
                        edge_type: ChunkEdgeType::Tests,
                        file_path: file_path.to_string(),
                    });
                    break;
                }
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Parser;
    use std::path::Path;

    fn parse(path: &str, content: &str) -> Vec<Chunk> {
        let mut parser = Parser::new().unwrap();
        parser.parse_file(Path::new(path), content).unwrap()
    }

    #[test]
    fn rust_test_prefix_links_to_production_fn() {
        let content = "\
fn parse_config() -> u32 { 1 }

fn test_parse_config() { assert_eq!(parse_config(), 1); }

fn unrelated_test() {}
";
        let chunks = parse("lib.rs", content);
        let edges = extract_test_edges("lib.rs", &chunks);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_name, "test_parse_config");
        assert_eq!(edges[0].target_name, "parse_config");
        assert_eq!(edges[0].edge_type, ChunkEdgeType::Tests);
    }

    #[test]
    fn suffix_and_camel_case_conventions() {
        assert_eq!(tested_names("parser_test"), vec!["parser"]);
        assert_eq!(tested_names("TestWidget"), vec!["Widget", "widget"]);
        assert_eq!(tested_names("testWidget"), vec!["Widget", "widget"]);
        assert!(tested_names("integration").is_empty());
        assert!(tested_names("test_").is_empty());
    }

    #[test]
    fn no_self_edge_and_no_dangling_target() {
        // `test_missing` has no production counterpart — no edge.
        let content = "fn test_missing() {}\n";
        let chunks = parse("lib.rs", content);
        assert!(extract_test_edges("lib.rs", &chunks).is_empty());
    }
}
