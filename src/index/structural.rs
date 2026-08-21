//! Deterministic structural edges between chunks of a single file.
//!
//! Derived purely from the chunk list the parser already produced — no
//! re-parse, no language-specific code:
//!
//! - `NextChunk`: each chunk points to the chunk that follows it in
//!   document (pre-order) order.
//! - `PartOf`: child → parent containment. For code and standalone
//!   markdown blocks this is line-range nesting (fn inside impl, table
//!   inside its section). Markdown sections *tile* the document (a
//!   section's range ends at the next heading of any level), so section
//!   hierarchy is recovered from breadcrumb names instead: a section
//!   named `"A > B"` is part of the nearest preceding section named `"A"`.

use crate::types::{Chunk, ChunkEdge, ChunkEdgeType, ChunkType};

/// Separator the markdown chunker uses when joining the header stack
/// into a breadcrumb section name (see `chunk_markdown`).
const BREADCRUMB_SEP: &str = " > ";

/// Extract `NextChunk` and `PartOf` edges for one file's chunks.
///
/// Works for any file type. Files with fewer than two chunks produce no
/// adjacency edges; chunks without a resolvable parent produce no
/// containment edge (degrades to "no edge", never a wrong edge).
pub fn extract_structural_edges(file_path: &str, chunks: &[Chunk]) -> Vec<ChunkEdge> {
    if chunks.len() < 2 {
        return Vec::new();
    }

    // Pre-order document order: by start line, containers before their
    // contents (larger span first), with type/name as deterministic
    // tie-breakers for the known same-span collision case (a markdown
    // table or code block spanning exactly its section's range).
    let mut ordered: Vec<&Chunk> = chunks.iter().collect();
    ordered.sort_by(|a, b| {
        a.start_line
            .cmp(&b.start_line)
            .then(b.end_line.cmp(&a.end_line))
            .then_with(|| format!("{:?}", a.chunk_type).cmp(&format!("{:?}", b.chunk_type)))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |edge: ChunkEdge| {
        if edge.source_chunk != edge.target_chunk
            && seen.insert((
                edge.source_chunk.clone(),
                edge.target_chunk.clone(),
                edge.edge_type,
            ))
        {
            edges.push(edge);
        }
    };

    // NextChunk: chain consecutive chunks in document order.
    for pair in ordered.windows(2) {
        push(make_edge(
            pair[0],
            pair[1],
            ChunkEdgeType::NextChunk,
            file_path,
        ));
    }

    // PartOf: one parent per chunk.
    for (i, chunk) in ordered.iter().enumerate() {
        let parent = if chunk.chunk_type == ChunkType::Section {
            breadcrumb_parent(chunk, &ordered[..i])
        } else {
            containing_parent(chunk, &ordered)
        };
        if let Some(parent) = parent {
            push(make_edge(chunk, parent, ChunkEdgeType::PartOf, file_path));
        }
    }

    edges
}

/// Smallest strictly-containing span among the other chunks.
fn containing_parent<'a>(chunk: &Chunk, ordered: &[&'a Chunk]) -> Option<&'a Chunk> {
    ordered
        .iter()
        .filter(|p| {
            p.id != chunk.id
                && p.start_line <= chunk.start_line
                && chunk.end_line <= p.end_line
                && (p.start_line, p.end_line) != (chunk.start_line, chunk.end_line)
        })
        .min_by_key(|p| p.end_line - p.start_line)
        .copied()
}

/// Parent of a markdown section via its breadcrumb name: strip the last
/// `" > "` segment and find the nearest preceding section with exactly
/// that name. Top-level sections (no separator) have no parent.
fn breadcrumb_parent<'a>(chunk: &Chunk, preceding: &[&'a Chunk]) -> Option<&'a Chunk> {
    let name = chunk.name.as_deref()?;
    let (parent_name, _) = name.rsplit_once(BREADCRUMB_SEP)?;
    preceding
        .iter()
        .rev()
        .find(|p| p.chunk_type == ChunkType::Section && p.name.as_deref() == Some(parent_name))
        .copied()
}

fn make_edge(
    source: &Chunk,
    target: &Chunk,
    edge_type: ChunkEdgeType,
    file_path: &str,
) -> ChunkEdge {
    ChunkEdge {
        source_chunk: source.id.clone(),
        target_chunk: target.id.clone(),
        source_name: source.name.clone().unwrap_or_default(),
        target_name: target.name.clone().unwrap_or_default(),
        edge_type,
        file_path: file_path.to_string(),
    }
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

    fn edges_of(chunks: &[Chunk]) -> Vec<ChunkEdge> {
        extract_structural_edges("test-file", chunks)
    }

    fn find<'a>(chunks: &'a [Chunk], name: &str) -> &'a Chunk {
        chunks
            .iter()
            .find(|c| c.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no chunk named {:?}", name))
    }

    fn has_edge(edges: &[ChunkEdge], source: &Chunk, target: &Chunk, t: ChunkEdgeType) -> bool {
        edges
            .iter()
            .any(|e| e.source_chunk == source.id && e.target_chunk == target.id && e.edge_type == t)
    }

    const NESTED_MD: &str = "\
intro text before any heading

# Top

top body

## Alpha

alpha body

### Alpha Sub

| a | b |
|---|---|
| 1 | 2 |

## Beta

```rust
fn beta() {}
```
";

    #[test]
    fn markdown_section_hierarchy_via_breadcrumbs() {
        let chunks = parse("doc.md", NESTED_MD);
        let edges = edges_of(&chunks);

        let top = find(&chunks, "Top");
        let alpha = find(&chunks, "Top > Alpha");
        let sub = find(&chunks, "Top > Alpha > Alpha Sub");
        let beta = find(&chunks, "Top > Beta");

        assert!(has_edge(&edges, alpha, top, ChunkEdgeType::PartOf));
        assert!(has_edge(&edges, sub, alpha, ChunkEdgeType::PartOf));
        assert!(has_edge(&edges, beta, top, ChunkEdgeType::PartOf));
        // Top-level heading and preamble have no parent
        let preamble = find(&chunks, "Preamble");
        assert!(!edges.iter().any(|e| e.edge_type == ChunkEdgeType::PartOf
            && (e.source_chunk == top.id || e.source_chunk == preamble.id)));
    }

    #[test]
    fn markdown_blocks_belong_to_their_section() {
        let chunks = parse("doc.md", NESTED_MD);
        let edges = edges_of(&chunks);

        let sub = find(&chunks, "Top > Alpha > Alpha Sub");
        let beta = find(&chunks, "Top > Beta");
        let table = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::Table)
            .unwrap();
        let code = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::CodeBlock)
            .unwrap();

        assert!(has_edge(&edges, table, sub, ChunkEdgeType::PartOf));
        assert!(has_edge(&edges, code, beta, ChunkEdgeType::PartOf));
    }

    #[test]
    fn markdown_adjacency_is_document_order() {
        // chunk_markdown appends Table/CodeBlock chunks in a second pass,
        // so the returned Vec is NOT document order; the edges must be.
        let chunks = parse("doc.md", NESTED_MD);
        let edges = edges_of(&chunks);

        let next: Vec<&ChunkEdge> = edges
            .iter()
            .filter(|e| e.edge_type == ChunkEdgeType::NextChunk)
            .collect();
        assert_eq!(next.len(), chunks.len() - 1);

        // Follow the chain and confirm start_lines never decrease
        let by_id = |id: &str| chunks.iter().find(|c| c.id == id).unwrap();
        for e in &next {
            assert!(
                by_id(&e.source_chunk).start_line <= by_id(&e.target_chunk).start_line,
                "adjacency goes backwards: {} -> {}",
                e.source_name,
                e.target_name
            );
        }

        // The table (mid-document, appended last by the chunker) is inside
        // the chain, not bolted onto the end.
        let table = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::Table)
            .unwrap();
        assert!(next.iter().any(|e| e.target_chunk == table.id));
        assert!(next.iter().any(|e| e.source_chunk == table.id));
    }

    #[test]
    fn code_nesting_via_line_ranges() {
        let content = "\
struct Foo;

impl Foo {
    fn alpha(&self) {}

    fn beta(&self) {}
}
";
        let chunks = parse("lib.rs", content);
        let edges = edges_of(&chunks);

        let imp = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::Impl)
            .unwrap();
        let alpha = find(&chunks, "alpha");
        let beta = find(&chunks, "beta");

        assert!(has_edge(&edges, alpha, imp, ChunkEdgeType::PartOf));
        assert!(has_edge(&edges, beta, imp, ChunkEdgeType::PartOf));
        // Pre-order adjacency: impl comes before its first method
        assert!(has_edge(&edges, imp, alpha, ChunkEdgeType::NextChunk));
        assert!(has_edge(&edges, alpha, beta, ChunkEdgeType::NextChunk));
    }

    #[test]
    fn degenerate_cases() {
        // Single chunk: no edges
        let chunks = parse("one.md", "just one paragraph, no headings\n");
        assert_eq!(chunks.len(), 1);
        assert!(edges_of(&chunks).is_empty());

        // Empty: no edges
        assert!(extract_structural_edges("x", &[]).is_empty());
    }

    #[test]
    fn identical_ids_produce_no_self_loops() {
        // Two chunks sharing (path, start, end) share an ID — the known
        // collision case. No self-loop edges, and output is deterministic.
        let chunks = parse("doc.md", NESTED_MD);
        let mut doubled = chunks.clone();
        doubled.extend(chunks.iter().cloned());
        let edges = edges_of(&doubled);
        assert!(edges.iter().all(|e| e.source_chunk != e.target_chunk));
        assert_eq!(edges, {
            let again = edges_of(&doubled);
            again
        });
    }

    #[test]
    fn duplicate_sibling_headings_resolve_to_nearest_preceding() {
        let content = "\
# A

## B

first b body

# A2

content
";
        let chunks = parse("doc.md", content);
        let edges = edges_of(&chunks);
        let b = find(&chunks, "A > B");
        let a = find(&chunks, "A");
        assert!(has_edge(&edges, b, a, ChunkEdgeType::PartOf));
    }
}
