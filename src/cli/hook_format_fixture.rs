    // Helper to create a standard test bundle for format mode tests.
    fn make_format_test_bundle() -> ContextBundle {
        ContextBundle {
            capture: None,
            query: "auth handler".to_string(),
            files: vec![ContextFile {
                path: "src/auth.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/auth.rs"),
                score: 0.85,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    id: String::new(),
                    name: Some("authenticate".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 10,
                    end_line: 25,
                    score: 0.85,
                    match_type: Some(MatchType::Hybrid),
                    content: Some("fn authenticate() {\n    // check token\n}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 16,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                structural_additions: 0,
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 0,
                top_semantic_score: 0.85,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        }
    }
