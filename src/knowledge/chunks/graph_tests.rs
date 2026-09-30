//! Named-graph publication (aegis-86f2v7): routing, ROOT isolation, source-scoped replace.

use super::tests::chunk;
use super::*;

#[test]
fn named_graph_push_lands_in_that_graph_and_leaves_root_untouched() {
    // aegis-86f2v7: third-party code published to a named graph must not touch ROOT,
    // and a re-push must replace within that graph only.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".bobbin/quipu/quipu.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let graph = "urn:test:graph:third-party-code";
    let g = quipu::Store::open(db.to_str().unwrap())
        .unwrap()
        .graph_create(graph)
        .unwrap();

    let two = vec![
        chunk("h1", "docs/guide.md", 1, Some("Intro")),
        chunk("h2", "docs/guide.md", 7, Some("Setup")),
    ];
    push_chunks_to_quipu(&two, &[], "r", dir.path(), Some(graph)).expect("graph push");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    assert!(
        store.current_facts().unwrap().is_empty(),
        "ROOT must stay empty"
    );
    let n_two = store.current_facts_in_graph(g).unwrap().len();
    assert!(n_two > 0, "facts must land in the named graph");
    drop(store);

    let one = vec![chunk("h1", "docs/guide.md", 1, Some("Intro"))];
    push_chunks_to_quipu(&one, &[], "r", dir.path(), Some(graph)).expect("second graph push");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    let n_one = store.current_facts_in_graph(g).unwrap().len();
    assert!(
        n_one > 0 && n_one < n_two,
        "re-push must retract the vanished chunk IN the graph"
    );
    assert!(
        store.current_facts().unwrap().is_empty(),
        "ROOT still empty after the replace"
    );
}

#[test]
fn unregistered_graph_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let chunks = vec![chunk("h1", "docs/guide.md", 1, Some("Intro"))];
    let err = push_chunks_to_quipu(
        &chunks,
        &[],
        "r",
        dir.path(),
        Some("urn:test:graph:never-registered"),
    )
    .expect_err("an unregistered target graph must be refused");
    assert!(
        err.to_string().contains("not usable"),
        "unexpected error: {err}"
    );
    let store =
        quipu::Store::open(dir.path().join(".bobbin/quipu/quipu.db").to_str().unwrap()).unwrap();
    assert!(
        store.current_facts().unwrap().is_empty(),
        "nothing may fall back into ROOT"
    );
}

#[test]
fn graph_publish_replaces_only_its_own_source_not_the_whole_graph() {
    // aegis-86f2v7 condition (sattler): the chunk snapshot may share a named graph with
    // facts from another producer (an issues graph). replace_snapshot must retract only
    // bobbin's own snapshot, so the foreign facts are counted EXACTLY before and after.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".bobbin/quipu/quipu.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let graph = "urn:test:graph:shared-knowledge";
    let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    let g = store.graph_create(graph).unwrap();
    let foreign = (1..=5)
        .map(|i| {
            format!(
                "<urn:test:issue:{i}> <http://www.w3.org/2000/01/rdf-schema#label> \"issue {i}\" ."
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    quipu::tool_knot(
        &mut store,
        &serde_json::json!({"turtle": foreign, "actor": "issue-ingest", "source": "issue-ingest", "graph": graph}),
    )
    .expect("seed foreign facts");
    let foreign_ids: Vec<i64> = (1..=5)
        .map(|i| {
            store
                .lookup(&format!("urn:test:issue:{i}"))
                .unwrap()
                .expect("interned")
        })
        .collect();
    let count_foreign = |s: &quipu::Store| {
        s.current_facts_in_graph(g)
            .unwrap()
            .iter()
            .filter(|f| foreign_ids.contains(&f.entity))
            .count()
    };
    let before = count_foreign(&store);
    assert_eq!(before, 5, "control: the seed must be visible");
    drop(store);

    let two = vec![
        chunk("h1", "docs/guide.md", 1, Some("Intro")),
        chunk("h2", "docs/guide.md", 7, Some("Setup")),
    ];
    push_chunks_to_quipu(&two, &[], "r", dir.path(), Some(graph)).expect("first publish");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    assert_eq!(
        count_foreign(&store),
        before,
        "first publish must not touch foreign facts"
    );
    drop(store);

    let one = vec![chunk("h1", "docs/guide.md", 1, Some("Intro"))];
    push_chunks_to_quipu(&one, &[], "r", dir.path(), Some(graph)).expect("re-publish");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    assert_eq!(
        count_foreign(&store),
        before,
        "a replacing re-publish must not touch them either"
    );
}

#[test]
fn multi_line_literal_publishes_and_round_trips() {
    // aegis-86f2v7.1: a heading carrying a raw newline made the whole gascity
    // snapshot fail with "Line jumps are not allowed in string literals".
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".bobbin/quipu/quipu.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let graph = "urn:test:graph:escaping";
    let g = quipu::Store::open(db.to_str().unwrap())
        .unwrap()
        .graph_create(graph)
        .unwrap();

    let heading = "Line one\nline \"two\"\r\n\ttabbed \\ end";
    let chunks = vec![chunk("h1", "docs/multi.md", 1, Some(heading))];
    push_chunks_to_quipu(&chunks, &[], "r", dir.path(), Some(graph))
        .expect("a multi-line heading must publish");

    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    let facts = format!("{:?}", store.current_facts_in_graph(g).unwrap());
    assert!(
        facts.contains(&format!("{heading:?}").trim_matches('"').to_string()),
        "the stored value must be the original text, not its escaped form: {facts}"
    );
}
