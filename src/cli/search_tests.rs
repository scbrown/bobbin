//! Tests for the search command internals (sidecar of `search.rs`).

use super::*;

#[test]
fn test_parse_chunk_type_valid() {
    assert_eq!(parse_chunk_type("function").unwrap(), ChunkType::Function);
    assert_eq!(parse_chunk_type("func").unwrap(), ChunkType::Function);
    assert_eq!(parse_chunk_type("fn").unwrap(), ChunkType::Function);
    assert_eq!(parse_chunk_type("method").unwrap(), ChunkType::Method);
    assert_eq!(parse_chunk_type("class").unwrap(), ChunkType::Class);
    assert_eq!(parse_chunk_type("struct").unwrap(), ChunkType::Struct);
    assert_eq!(parse_chunk_type("enum").unwrap(), ChunkType::Enum);
    assert_eq!(parse_chunk_type("interface").unwrap(), ChunkType::Interface);
    assert_eq!(parse_chunk_type("module").unwrap(), ChunkType::Module);
    assert_eq!(parse_chunk_type("mod").unwrap(), ChunkType::Module);
    assert_eq!(parse_chunk_type("impl").unwrap(), ChunkType::Impl);
    assert_eq!(parse_chunk_type("trait").unwrap(), ChunkType::Trait);
    assert_eq!(parse_chunk_type("commit").unwrap(), ChunkType::Commit);
    assert_eq!(parse_chunk_type("other").unwrap(), ChunkType::Other);
}

#[test]
fn test_parse_chunk_type_case_insensitive() {
    assert_eq!(parse_chunk_type("FUNCTION").unwrap(), ChunkType::Function);
    assert_eq!(parse_chunk_type("Function").unwrap(), ChunkType::Function);
    assert_eq!(parse_chunk_type("STRUCT").unwrap(), ChunkType::Struct);
    assert_eq!(parse_chunk_type("Trait").unwrap(), ChunkType::Trait);
}

#[test]
fn test_parse_chunk_type_invalid() {
    assert!(parse_chunk_type("invalid").is_err());
    assert!(parse_chunk_type("").is_err());
    assert!(parse_chunk_type("functon").is_err());
}

#[test]
fn test_truncate_content_short() {
    let content = "short content";
    let result = truncate_content(content, 100);
    assert_eq!(result, "short content");
}

#[test]
fn test_truncate_content_long() {
    let content = "This is a very long piece of content that should be truncated";
    let result = truncate_content(content, 20);
    assert_eq!(result, "This is a very long...");
}

#[test]
fn test_truncate_content_exact() {
    let content = "exact";
    let result = truncate_content(content, 5);
    assert_eq!(result, "exact");
}

#[test]
fn test_truncate_content_unicode() {
    let content = "こんにちは世界";
    let result = truncate_content(content, 3);
    assert_eq!(result, "こんに...");
}

#[test]
fn test_search_output_serialization() {
    use crate::types::MatchType;

    let results = vec![SearchResult {
        chunk: crate::types::Chunk {
            id: "test-id".to_string(),
            file_path: "src/main.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("test_fn".to_string()),
            start_line: 1,
            end_line: 10,
            content: "fn test_fn() {}".to_string(),
            language: "rust".to_string(),
            tags: String::new(),
        },
        score: 0.95,
        match_type: Some(MatchType::Semantic),
        indexed_at: None,
        repo: None,
    }];

    let output = SearchOutput {
        query: "test query".to_string(),
        mode: "hybrid".to_string(),
        r#type: Some("function".to_string()),
        limit: 10,
        count: 1,
        results: results
            .iter()
            .map(|r| SearchResultOutput {
                file_path: r.chunk.file_path.clone(),
                name: r.chunk.name.clone(),
                chunk_type: r.chunk.chunk_type.to_string(),
                source: source_kind(&r.chunk.chunk_type).to_string(),
                repo: r.repo.clone(),
                start_line: r.chunk.start_line,
                end_line: r.chunk.end_line,
                score: r.score,
                match_type: r.match_type.map(|mt| match mt {
                    MatchType::Semantic => "semantic".to_string(),
                    MatchType::Keyword => "keyword".to_string(),
                    MatchType::Hybrid => "hybrid".to_string(),
                }),
                language: r.chunk.language.clone(),
                content_preview: Some(truncate_content(&r.chunk.content, 200)),
            })
            .collect(),
    };

    let json = serde_json::to_string(&output).unwrap();
    assert!(json.contains("\"query\":\"test query\""));
    assert!(json.contains("\"mode\":\"hybrid\""));
    assert!(json.contains("\"file_path\":\"src/main.rs\""));
    assert!(json.contains("\"chunk_type\":\"function\""));
    assert!(json.contains("\"score\":0.95"));
    assert!(json.contains("\"match_type\":\"semantic\""));
}

// -- Query interpretation wiring (#50 item 1) --

use crate::search::query::parse;

#[test]
fn inline_repo_filter_beats_the_flag() {
    // `bobbin search 'repo:aegis foo' --repo other` -> aegis.
    assert_eq!(
        resolve_repo(&parse("repo:aegis foo"), Some("other")).as_deref(),
        Some("aegis")
    );
    // With no inline filter the flag still applies.
    assert_eq!(
        resolve_repo(&parse("foo"), Some("other")).as_deref(),
        Some("other")
    );
    // Neither: no repo filter at all, not an empty-string one.
    assert_eq!(resolve_repo(&parse("foo"), None), None);
}

#[test]
fn multi_value_and_negated_repo_stay_in_sql() {
    // `repo:a,b` and `-repo:x` cannot be expressed by the single-repo
    // parameter; they must fall through to filters_to_sql instead of
    // silently collapsing to the first value or to a positive match.
    assert_eq!(resolve_repo(&parse("repo:a,b foo"), None), None);
    assert_eq!(resolve_repo(&parse("-repo:x foo"), None), None);
    let sql = crate::search::query::filters_to_sql(&parse("repo:a,b foo").filters);
    assert!(!sql.is_empty(), "multi-value repo must still produce SQL");
}

#[test]
fn inline_group_beats_the_flag() {
    assert_eq!(
        resolve_group(&parse("group:infra foo"), Some("other")).as_deref(),
        Some("infra")
    );
    assert_eq!(
        resolve_group(&parse("foo"), Some("other")).as_deref(),
        Some("other")
    );
    assert_eq!(resolve_group(&parse("foo"), None), None);
}

#[test]
fn search_text_strips_filters_but_never_goes_empty() {
    // Filters must not also be matched as literal words: no chunk contains
    // the string "repo:aegis".
    assert_eq!(
        search_text_for(&parse("repo:aegis error"), "repo:aegis error"),
        "error"
    );
    // A filters-only query falls back to the raw string rather than
    // searching for nothing.
    assert_eq!(
        search_text_for(&parse("repo:aegis"), "repo:aegis"),
        "repo:aegis"
    );
    // Phrases survive into the text query.
    assert_eq!(
        search_text_for(&parse("repo:aegis \"error handling\""), "x"),
        "\"error handling\""
    );
}

#[test]
fn cli_and_http_read_a_query_identically() {
    // The property #50 item 1 is about: one query, one interpretation.
    // Both surfaces call `query::parse`, so asserting on the parse is
    // asserting on both.
    let q = "+context -assembler repo:aegis lang:rust /handler/ a OR b";
    let p = parse(q);
    assert_eq!(p.required_terms, vec!["context"]);
    assert_eq!(p.negated_terms, vec!["assembler"]);
    assert_eq!(p.regex_patterns, vec!["handler"]);
    assert!(p.has_or);
    assert_eq!(resolve_repo(&p, None).as_deref(), Some("aegis"));
    assert!(
        crate::search::query::filters_to_sql(&p.filters)
            .iter()
            .any(|c| c.contains("language")),
        "lang: must reach SQL"
    );
}
