use super::*;
use crate::index::parser::Parser;
use std::collections::HashSet;

#[test]
fn ordinary_parser_ids_remain_unchanged() {
    let mut parser = Parser::new().unwrap();
    let chunks = parser
        .parse_file(
            Path::new("ordinary.ts"),
            "function first() {\n  return 1;\n}\nfunction second() {\n  return 2;\n}",
        )
        .unwrap();
    assert_eq!(chunks.len(), 2);
    // Literal IDs recorded from the original path:start:end hashing scheme.
    assert_eq!(chunks[0].id, "fa591a925588e4c4");
    assert_eq!(chunks[1].id, "08789ee42cd1e205");
}

#[test]
fn same_line_callbacks_keep_distinct_order_and_adjacency() {
    let source = "function nested(ms) { return ms.some(m => m.hs.some(h => h.ok)); }\n\
                  function repeated(xs) { return xs.filter(c => c.ok).map(c => c.ok); }";
    let mut parser = Parser::new().unwrap();
    let chunks = parser
        .parse_file(Path::new("callbacks.ts"), source)
        .unwrap();
    assert_eq!(
        chunks.iter().map(|c| &c.id).collect::<HashSet<_>>().len(),
        chunks.len(),
        "parser IDs must distinguish every same-line span before edges are built"
    );
    for line in [1, 2] {
        assert_eq!(
            chunks
                .iter()
                .filter(|c| c.start_line == line && c.name.is_none())
                .count(),
            2,
            "fixture must extract both unnamed callbacks"
        );
    }
    let edges = parser.extract_chunk_edges(Path::new("callbacks.ts"), source, &chunks);
    let turtle = generate_chunk_turtle(&chunks, &edges, "fixture");
    assert_unique_chunks(&turtle, chunks.len());
    for line in turtle
        .lines()
        .filter(|line| line.contains(" bobbin:nextChunk "))
    {
        let (source, target) = line.split_once(" bobbin:nextChunk ").unwrap();
        assert_ne!(
            source,
            target.trim_end_matches(" ."),
            "adjacent spans must not collapse to a self-edge"
        );
    }
    assert_eq!(
        turtle
            .lines()
            .filter(|line| line.contains(" bobbin:nextChunk "))
            .count(),
        chunks.len() - 1
    );
    let mut reversed = chunks.clone();
    reversed.reverse();
    assert_eq!(turtle, generate_chunk_turtle(&reversed, &edges, "fixture"));
}

#[test]
fn demoted_symbols_share_coordinates_without_sharing_identity() {
    let mut parser = Parser::new().unwrap();
    let chunks = parser
        .parse_file(
            Path::new("callbacks.ts"),
            "function f() {} function f() {} function f() {} (() => 1)();",
        )
        .unwrap();
    assert!(
        chunks.len() >= 4,
        "fixture must exercise multiple demotions and an anonymous span"
    );
    let turtle = generate_chunk_turtle(&chunks, &[], "fixture");
    assert_unique_chunks(&turtle, chunks.len());
    assert!(
        turtle.contains("Demoting"),
        "must exercise governed-identity collision"
    );
}

fn assert_unique_chunks(turtle: &str, expected: usize) {
    let subjects: Vec<_> = turtle
        .lines()
        .filter_map(|line| line.split_once(" a bobbin:Chunk").map(|(iri, _)| iri))
        .collect();
    assert_eq!(subjects.len(), expected, "no source spans may be lost");
    assert_eq!(
        subjects.iter().collect::<HashSet<_>>().len(),
        expected,
        "each span must have its own subject, hence only one chunkOrder value"
    );
    assert_eq!(
        turtle
            .lines()
            .filter(|line| line.contains("bobbin:chunkOrder "))
            .count(),
        expected
    );
}

#[test]
fn emitted_callback_snapshot_passes_cardinality_validation() {
    let mut parser = Parser::new().unwrap();
    let chunks = parser
        .parse_file(
            Path::new("callbacks.ts"),
            "function f(xs) { return xs.filter(c => c.ok).map(c => c.ok); }",
        )
        .unwrap();
    let turtle = generate_chunk_turtle(&chunks, &[], "fixture");
    let shapes = format!(
        r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix b: <{ONTOLOGY_NS}> .
        <urn:chunk-order-shape> a sh:NodeShape ;
            sh:targetClass b:Chunk ;
            sh:property [ sh:path b:chunkOrder ; sh:minCount 1 ; sh:maxCount 1 ] .
    "#
    );
    let validate = |data: &str| {
        quipu::tool_validate(&serde_json::json!({
            "shapes": shapes, "data": data,
        }))
        .unwrap()
    };
    assert_eq!(validate(&turtle)["conforms"], true);
    // Positive control for the validator: a second order on one emitted subject
    // recreates the actual refusal, even though the rest of the snapshot is valid.
    let subject = turtle
        .lines()
        .find_map(|line| {
            line.split_once(" a bobbin:Chunk")
                .map(|(subject, _)| subject)
        })
        .unwrap();
    let broken = format!("{turtle}\n{subject} <{ONTOLOGY_NS}chunkOrder> 999 .\n");
    assert_eq!(validate(&broken)["conforms"], false);
}
