use std::collections::{HashMap, HashSet};
use serde_json::json;
use super::*;
use super::types::*;
use super::util::*;
use super::install::*;
use super::git_hook::*;
use super::state::*;
use super::ledger::*;
use super::format::*;
use super::hot_topics::*;
use crate::search::context::*;
use crate::types::{ChunkType, MatchType, classify_file};
    #[test]
    fn test_hook_config_output_serialization() {
        let output = HookStatusOutput {
            hooks_installed: false,
            git_hook_installed: false,
            config: HookConfigOutput {
                threshold: 0.5,
                budget: 150,
                content_mode: "preview".to_string(),
                min_prompt_length: 10,
                gate_threshold: 0.75,
                dedup_enabled: true,
            },
            injection_count: 42,
            last_injection_time: Some("2026-02-08T10:30:00Z".to_string()),
            last_session_id: Some("a1b2c3d4e5f6a7b8".to_string()),
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"threshold\":0.5"));
        assert!(json.contains("\"budget\":150"));
        assert!(json.contains("\"content_mode\":\"preview\""));
        assert!(json.contains("\"gate_threshold\":0.75"));
        assert!(json.contains("\"dedup_enabled\":true"));
        assert!(json.contains("\"injection_count\":42"));
        assert!(json.contains("\"last_injection_time\":\"2026-02-08T10:30:00Z\""));
        assert!(json.contains("\"last_session_id\":\"a1b2c3d4e5f6a7b8\""));
    }

    #[test]
    fn test_hook_status_output_no_state() {
        let output = HookStatusOutput {
            hooks_installed: true,
            git_hook_installed: false,
            config: HookConfigOutput {
                threshold: 0.5,
                budget: 150,
                content_mode: "preview".to_string(),
                min_prompt_length: 10,
                gate_threshold: 0.75,
                dedup_enabled: true,
            },
            injection_count: 0,
            last_injection_time: None,
            last_session_id: None,
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"injection_count\":0"));
        assert!(json.contains("\"last_injection_time\":null"));
        assert!(json.contains("\"last_session_id\":null"));
    }

    #[test]
    fn test_hook_input_deserialization() {
        let json = r#"{"session_id":"abc","prompt":"find auth code","cwd":"/home/user/project","permission_mode":"default","hook_event_name":"UserPromptSubmit"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.prompt, "find auth code");
        assert_eq!(input.cwd, "/home/user/project");
    }

    #[test]
    fn test_hook_input_missing_fields() {
        // Extra fields are ignored, missing optional fields get defaults
        let json = r#"{"prompt":"hello"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.prompt, "hello");
        assert!(input.cwd.is_empty());
    }

    #[test]
    fn test_hook_input_empty_object() {
        let json = r#"{}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert!(input.prompt.is_empty());
        assert!(input.cwd.is_empty());
    }

    #[test]
    fn test_find_bobbin_root_not_found() {
        let tmp = std::env::temp_dir().join("bobbin_test_no_root");
        std::fs::create_dir_all(&tmp).ok();
        assert!(find_bobbin_root(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_find_bobbin_root_direct() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();
        std::fs::write(bobbin_dir.join("config.toml"), "").unwrap();

        let found = find_bobbin_root(tmp.path());
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_find_bobbin_root_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();
        std::fs::write(bobbin_dir.join("config.toml"), "").unwrap();

        let child = tmp.path().join("src").join("lib");
        std::fs::create_dir_all(&child).unwrap();

        let found = find_bobbin_root(&child);
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn test_format_context_empty_bundle() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 0,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        assert!(result.contains("0 relevant files"));
    }

    #[test]
    fn test_format_context_with_results() {
        let bundle = ContextBundle {
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
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.5, true, None, "standard");
        assert!(result.contains("src/auth.rs:10-25"));
        assert!(result.contains("authenticate"));
        assert!(result.contains("fn authenticate()"));
        assert!(result.contains("score 0.85"));
    }

    #[test]
    fn test_format_context_with_injection_id() {
        let bundle = ContextBundle {
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
                    name: Some("authenticate".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 10,
                    end_line: 25,
                    score: 0.85,
                    match_type: Some(MatchType::Hybrid),
                    content: Some("fn authenticate() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
                pinned_lines: 0,
            },
            summary: ContextSummary {
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
        };

        // With injection_id
        let result = format_context_for_injection(&bundle, 0.0, true, Some("inj-abc12345"), "standard");
        assert!(result.contains("[injection_id: inj-abc12345]"));
        assert!(result.contains("1 relevant files"));

        // Without injection_id (backward compat)
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        assert!(!result.contains("injection_id"));
        assert!(result.contains("1 relevant files"));
    }

    #[test]
    fn test_generate_context_injection_id() {
        let id1 = generate_context_injection_id("hello world");
        let id2 = generate_context_injection_id("hello world");
        // Each call should produce a unique ID (timestamp differs)
        assert!(id1.starts_with("inj-"));
        assert_eq!(id1.len(), 12); // "inj-" + 8 hex chars
        assert!(id2.starts_with("inj-"));
        // IDs should differ (nanosecond timestamp)
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_format_context_threshold_filters() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/low.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/low.rs"),
                score: 0.3,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    name: Some("low_score_fn".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 5,
                    score: 0.3,
                    match_type: None,
                    content: Some("fn low() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 5,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        // With high threshold, chunk content should be filtered out
        let result = format_context_for_injection(&bundle, 0.5, true, None, "standard");
        assert!(!result.contains("low_score_fn"));
    }

    #[test]
    fn test_session_start_input_parsing() {
        let json = r#"{"source": "compact", "cwd": "/tmp/test", "session_id": "abc"}"#;
        let input: SessionStartInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.source, "compact");
        assert_eq!(input.cwd, "/tmp/test");
    }

    #[test]
    fn test_session_start_input_defaults() {
        let json = r#"{}"#;
        let input: SessionStartInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.source, "");
        assert_eq!(input.cwd, "");
    }

    #[test]
    fn test_hook_response_serialization() {
        let response = HookResponse {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "SessionStart".to_string(),
                additional_context: "test context".to_string(),
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("hookSpecificOutput"));
        assert!(json.contains("hookEventName"));
        assert!(json.contains("additionalContext"));
        assert!(json.contains("SessionStart"));
        assert!(json.contains("test context"));
    }

    #[test]
    fn test_format_session_context_modified_files() {
        let modified = vec!["src/main.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("## Working Context"));
        assert!(result.contains("### Modified files"));
        assert!(result.contains("- src/main.rs"));
    }

    #[test]
    fn test_format_session_context_with_symbols() {
        let modified = vec!["src/auth.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![FileSymbolInfo {
            path: "src/auth.rs".to_string(),
            symbols: vec![
                SymbolInfo {
                    name: "validate_token".to_string(),

                },
                SymbolInfo {
                    name: "refresh_session".to_string(),

                },
            ],
        }];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("src/auth.rs (2 symbols: validate_token, refresh_session)"));
    }

    #[test]
    fn test_format_session_context_with_commits() {
        let modified: Vec<String> = vec![];
        let commits = vec![
            "a1b2c3d fix: token refresh race condition".to_string(),
            "d4e5f6g feat: add logout endpoint".to_string(),
        ];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Recent commits"));
        assert!(result.contains("- a1b2c3d fix: token refresh race condition"));
    }

    #[test]
    fn test_format_session_context_with_coupling() {
        let modified = vec!["src/auth.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled = vec![(
            "tests/auth_test.rs".to_string(),
            "src/auth.rs".to_string(),
            0.91,
        )];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Related files (via coupling)"));
        assert!(result.contains("tests/auth_test.rs (coupled with src/auth.rs, score: 0.91)"));
    }

    #[test]
    fn test_format_session_context_budget_enforcement() {
        let modified: Vec<String> = (0..100)
            .map(|i| format!("src/file_{}.rs", i))
            .collect();
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 10);
        let line_count = result.lines().count();
        // Budget of 10 — truncation message counts within budget
        assert!(line_count <= 10, "Expected <= 10 lines, got {}", line_count);
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_format_session_context_many_symbols_truncated() {
        let modified = vec!["src/big.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![FileSymbolInfo {
            path: "src/big.rs".to_string(),
            symbols: (0..8)
                .map(|i| SymbolInfo {
                    name: format!("fn_{}", i),

                })
                .collect(),
        }];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        // Should show 5 symbols + "..." indicator
        assert!(result.contains("8 symbols: fn_0, fn_1, fn_2, fn_3, fn_4, ..."));
    }

    #[test]
    fn test_format_session_context_recently_changed_separate() {
        // Modified files and recently changed files should appear in different sections
        let modified = vec!["src/modified.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols = vec![
            FileSymbolInfo {
                path: "src/modified.rs".to_string(),
                symbols: vec![SymbolInfo {
                    name: "mod_fn".to_string(),

                }],
            },
            FileSymbolInfo {
                path: "src/recent.rs".to_string(),
                symbols: vec![SymbolInfo {
                    name: "recent_fn".to_string(),

                }],
            },
        ];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("### Modified files"));
        assert!(result.contains("### Recently changed files"));
        assert!(result.contains("- src/recent.rs (1 symbols: recent_fn)"));
    }

    #[test]
    fn test_format_session_context_empty_produces_header_only() {
        let modified: Vec<String> = vec![];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        let result = format_session_context(&modified, &commits, &symbols, &coupled, 150);
        assert!(result.contains("## Working Context"));
        // Header line + trailing newline from blank line join
        assert!(result.lines().count() <= 2);
    }

    // --- Budget enforcement tests for inject-context formatter ---

    #[test]
    fn test_format_context_for_injection_respects_budget() {
        // Build a bundle with many chunks that would exceed a small budget
        let bundle = ContextBundle {
            query: "auth".to_string(),
            files: vec![
                ContextFile {
                    path: "src/a.rs".to_string(),
                    language: "rust".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("src/a.rs"),
                    score: 0.9,
                    coupled_to: vec![],
                    repo: None,
                    chunks: vec![
                        ContextChunk {
                            name: Some("fn_a".to_string()),
                            chunk_type: ChunkType::Function,
                            start_line: 1,
                            end_line: 10,
                            score: 0.9,
                            match_type: Some(MatchType::Hybrid),
                            content: Some("line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10".to_string()),
                        },
                        ContextChunk {
                            name: Some("fn_b".to_string()),
                            chunk_type: ChunkType::Function,
                            start_line: 20,
                            end_line: 30,
                            score: 0.8,
                            match_type: Some(MatchType::Hybrid),
                            content: Some("b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\nb9\nb10\nb11".to_string()),
                        },
                    ],
                },
            ],
            budget: BudgetInfo {
                max_lines: 15,
                used_lines: 21,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        let line_count = result.lines().count();
        // Must not exceed max_lines budget
        assert!(
            line_count <= 15,
            "Expected <= 15 lines, got {}:\n{}",
            line_count,
            result
        );
        // Should include at least the first chunk
        assert!(result.contains("fn_a"));
    }

    #[test]
    fn test_format_context_for_injection_score_format() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/x.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/x.rs"),
                score: 0.85,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    name: Some("fn_x".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 3,
                    score: 0.856,
                    match_type: None,
                    content: Some("fn x() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 3,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        // Score should be 2 decimal places
        assert!(result.contains("score 0.86"), "Expected 2-decimal score in: {}", result);
    }

    #[test]
    fn test_format_context_show_docs_false_excludes_doc_files() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![
                ContextFile {
                    path: "src/main.rs".to_string(),
                    language: "rust".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("src/main.rs"),
                    score: 0.9,
                    coupled_to: vec![],
                    repo: None,
                    chunks: vec![ContextChunk {
                        name: Some("main".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 1,
                        end_line: 5,
                        score: 0.9,
                        match_type: None,
                        content: Some("fn main() {}".to_string()),
                    }],
                },
                ContextFile {
                    path: "README.md".to_string(),
                    language: "markdown".to_string(),
                    relevance: FileRelevance::Direct,
                    category: classify_file("README.md"),
                    score: 0.8,
                    coupled_to: vec![],
                    repo: None,
                    chunks: vec![ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Module,
                        start_line: 1,
                        end_line: 10,
                        score: 0.8,
                        match_type: None,
                        content: Some("# My Project".to_string()),
                    }],
                },
            ],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 15,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 2,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 1,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        // show_docs=true should include both
        let with_docs = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        assert!(with_docs.contains("Source Files"), "Should have source section");
        assert!(with_docs.contains("Documentation"), "Should have doc section");
        assert!(with_docs.contains("README.md"));

        // show_docs=false should exclude documentation
        let without_docs = format_context_for_injection(&bundle, 0.0, false, None, "standard");
        assert!(without_docs.contains("Source Files"), "Should have source section");
        assert!(!without_docs.contains("Documentation"), "Should not have doc section");
        assert!(!without_docs.contains("README.md"), "Doc file should be excluded");
        assert!(without_docs.contains("src/main.rs"), "Source file should remain");
    }

    #[test]
    fn test_format_context_budget_zero() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    name: Some("fn_a".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 10,
                    score: 0.9,
                    match_type: None,
                    content: Some("fn a() {}".to_string()),
                }],
            }],
            budget: BudgetInfo {
                max_lines: 0,
                used_lines: 0,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        // Budget 0 — should not panic and should produce empty or minimal output
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        assert!(result.lines().count() <= 1, "Budget 0 should produce at most the header");
    }

    #[test]
    fn test_format_context_no_content() {
        // Test formatting when content is None (ContentMode::None)
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    name: Some("fn_a".to_string()),
                    chunk_type: ChunkType::Function,
                    start_line: 1,
                    end_line: 10,
                    score: 0.9,
                    match_type: None,
                    content: None,
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 1,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };
        let result = format_context_for_injection(&bundle, 0.0, true, None, "standard");
        // Should still have the chunk header with file:lines
        assert!(result.contains("src/a.rs:1-10"));
        assert!(result.contains("fn_a"));
    }

    // Helper to create a standard test bundle for format mode tests.
    fn make_format_test_bundle() -> ContextBundle {
        ContextBundle {
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

    #[test]
    fn test_format_mode_standard() {
        let bundle = make_format_test_bundle();
        let result = format_context_for_injection(&bundle, 0.0, true, Some("inj-test1"), "standard");
        assert!(result.contains("Bobbin found 1 relevant files"));
        assert!(result.contains("[injection_id: inj-test1]"));
        assert!(result.contains("=== Source Files ==="));
        assert!(result.contains("--- src/auth.rs:10-25"));
        assert!(result.contains("score 0.85"));
        assert!(result.contains("fn authenticate()"));
    }

    #[test]
    fn test_format_mode_minimal() {
        let bundle = make_format_test_bundle();
        let result = format_context_for_injection(&bundle, 0.0, true, Some("inj-test2"), "minimal");
        // Minimal: no section headers, no scores, no types
        assert!(result.contains("# Bobbin context"));
        assert!(result.contains("[injection_id: inj-test2]"));
        assert!(!result.contains("=== Source Files ==="));
        assert!(!result.contains("score 0.85"));
        assert!(result.contains("# src/auth.rs (lines 10-25)"));
        assert!(result.contains("fn authenticate()"));
    }

    #[test]
    fn test_format_mode_verbose() {
        let bundle = make_format_test_bundle();
        let result = format_context_for_injection(&bundle, 0.0, true, Some("inj-test3"), "verbose");
        assert!(result.contains("Bobbin found 1 relevant files"));
        assert!(result.contains("=== Source Files ==="));
        assert!(result.contains("--- src/auth.rs:10-25"));
        assert!(result.contains("score 0.85"));
        // Verbose adds explicit type/name line
        assert!(result.contains("// function authenticate"));
        assert!(result.contains("fn authenticate()"));
    }

    #[test]
    fn test_format_mode_xml() {
        let bundle = make_format_test_bundle();
        let result = format_context_for_injection(&bundle, 0.0, true, Some("inj-test4"), "xml");
        assert!(result.contains("<bobbin-context"));
        assert!(result.contains("injection_id=\"inj-test4\""));
        assert!(result.contains("</bobbin-context>"));
        assert!(result.contains("<file path=\"src/auth.rs\""));
        assert!(result.contains("lines=\"10-25\""));
        assert!(result.contains("score=\"0.85\""));
        assert!(result.contains("</file>"));
        assert!(result.contains("fn authenticate()"));
        // XML mode should NOT have section headers
        assert!(!result.contains("=== Source Files ==="));
    }

    #[test]
    fn test_format_search_chunk_all_modes() {
        let content = "fn main() {}\n";
        let standard = format_search_chunk("src/main.rs", 1, 5, " main", "function", 0.9, content, "", "standard");
        assert!(standard.contains("--- src/main.rs:1-5 main (function, score 0.90) ---"));

        let minimal = format_search_chunk("src/main.rs", 1, 5, " main", "function", 0.9, content, "", "minimal");
        assert!(minimal.contains("# src/main.rs (lines 1-5)"));
        assert!(!minimal.contains("score"));

        let xml = format_search_chunk("src/main.rs", 1, 5, " main", "function", 0.9, content, "", "xml");
        assert!(xml.contains("<file path=\"src/main.rs\""));
        assert!(xml.contains("name=\"main\""));
        assert!(xml.contains("</file>"));
    }

    #[test]
    fn test_format_session_context_very_small_budget() {
        let modified = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        // Budget of 3 — header + blank + 1 content line at most
        let result = format_session_context(&modified, &commits, &symbols, &coupled, 3);
        let line_count = result.lines().count();
        assert!(line_count <= 3, "Expected <= 3 lines, got {}:\n{}", line_count, result);
    }

    #[test]
    fn test_format_session_context_budget_zero() {
        let modified = vec!["src/a.rs".to_string()];
        let commits: Vec<String> = vec![];
        let symbols: Vec<FileSymbolInfo> = vec![];
        let coupled: Vec<(String, String, f32)> = vec![];

        // Budget of 0 — should still not panic
        let result = format_session_context(&modified, &commits, &symbols, &coupled, 0);
        // Should produce at most the truncation message
        assert!(result.lines().count() <= 1);
    }

    // --- Hook installer unit tests ---

    #[test]
    fn test_merge_hooks_into_empty_settings() {
        let mut settings = json!({});
        merge_hooks(&mut settings);

        assert!(settings.get("hooks").is_some());
        let hooks = &settings["hooks"];
        assert!(hooks.get("UserPromptSubmit").is_some());
        assert!(hooks.get("SessionStart").is_some());

        // Verify inject-context command
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        let cmd = ups[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "bobbin hook inject-context || true");

        // Verify session-context command
        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1);
        let cmd = ss[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "bobbin hook session-context || true");
        assert_eq!(ss[0]["matcher"].as_str().unwrap(), "compact");
    }

    #[test]
    fn test_merge_hooks_preserves_existing_hooks() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "other-tool inject",
                                "timeout": 5
                            }
                        ]
                    }
                ]
            },
            "other_key": "preserved"
        });

        merge_hooks(&mut settings);

        // other_key should still be there
        assert_eq!(settings["other_key"].as_str().unwrap(), "preserved");

        // UserPromptSubmit should have both the other tool AND bobbin
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2);
        assert_eq!(
            ups[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool inject"
        );
        assert_eq!(
            ups[1]["hooks"][0]["command"].as_str().unwrap(),
            "bobbin hook inject-context || true"
        );
    }

    #[test]
    fn test_merge_hooks_idempotent() {
        let mut settings = json!({});
        merge_hooks(&mut settings);
        merge_hooks(&mut settings); // Second time

        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1, "Should not duplicate bobbin hooks");

        let ss = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1, "Should not duplicate bobbin hooks");
    }

    #[test]
    fn test_is_bobbin_hook_group_true() {
        let group = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": "bobbin hook inject-context",
                    "timeout": 10
                }
            ]
        });
        assert!(is_bobbin_hook_group(&group));
    }

    #[test]
    fn test_is_bobbin_hook_group_with_fallback() {
        // Old-format hooks (without || true) should still be detected
        let group = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": "bobbin hook inject-context || true",
                    "timeout": 10
                }
            ]
        });
        assert!(is_bobbin_hook_group(&group));
    }

    #[test]
    fn test_is_bobbin_hook_group_false() {
        let group = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": "other-tool do-thing",
                    "timeout": 5
                }
            ]
        });
        assert!(!is_bobbin_hook_group(&group));
    }

    #[test]
    fn test_remove_bobbin_hooks_leaves_others() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool inject" }
                        ]
                    },
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ],
                "SessionStart": [
                    {
                        "matcher": "compact",
                        "hooks": [
                            { "type": "command", "command": "bobbin hook session-context" }
                        ]
                    }
                ]
            }
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(removed);

        // other-tool should remain
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert_eq!(
            ups[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool inject"
        );

        // SessionStart was only bobbin, so it should be removed entirely
        assert!(settings["hooks"].get("SessionStart").is_none());
    }

    #[test]
    fn test_remove_bobbin_hooks_cleans_empty_hooks_object() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ]
            },
            "other": true
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(removed);

        // hooks object should be fully removed
        assert!(settings.get("hooks").is_none());
        // other keys preserved
        assert_eq!(settings["other"].as_bool().unwrap(), true);
    }

    #[test]
    fn test_remove_bobbin_hooks_none_present() {
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool inject" }
                        ]
                    }
                ]
            }
        });

        let removed = remove_bobbin_hooks(&mut settings);
        assert!(!removed);

        // Nothing should change
        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
    }

    #[test]
    fn test_has_bobbin_hooks_true() {
        let settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "bobbin hook inject-context" }
                        ]
                    }
                ]
            }
        });
        assert!(has_bobbin_hooks(&settings));
    }

    #[test]
    fn test_has_bobbin_hooks_false() {
        let settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "other-tool" }
                        ]
                    }
                ]
            }
        });
        assert!(!has_bobbin_hooks(&settings));
    }

    #[test]
    fn test_has_bobbin_hooks_empty() {
        assert!(!has_bobbin_hooks(&json!({})));
    }

    #[test]
    fn test_read_settings_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings, json!({}));
    }

    #[test]
    fn test_read_settings_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.json");
        std::fs::write(&path, "").unwrap();
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings, json!({}));
    }

    #[test]
    fn test_read_settings_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("valid.json");
        std::fs::write(&path, r#"{"key": "value"}"#).unwrap();
        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_write_settings_creates_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("deep").join("nested").join("settings.json");
        let settings = json!({"test": true});
        write_settings(&path, &settings).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["test"].as_bool().unwrap(), true);
    }

    #[test]
    fn test_merge_hooks_preserves_unrelated_events() {
        // Events that bobbin doesn't use should be completely untouched
        let mut settings = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt tap guard pr-workflow" }
                        ],
                        "matcher": "Bash(gh pr create*)"
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt costs record" }
                        ],
                        "matcher": ""
                    }
                ],
                "PostToolUseFailure": [
                    {
                        "hooks": [
                            { "type": "command", "command": "dp record --source claude-code" }
                        ],
                        "matcher": ".*"
                    }
                ]
            }
        });

        merge_hooks(&mut settings);

        // Unrelated events preserved
        let hooks = &settings["hooks"];
        assert_eq!(hooks["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(
            hooks["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap(),
            "gt tap guard pr-workflow"
        );
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);

        // PostToolUseFailure: original dp hook + new bobbin hook
        assert_eq!(hooks["PostToolUseFailure"].as_array().unwrap().len(), 2);
        assert_eq!(
            hooks["PostToolUseFailure"][0]["hooks"][0]["command"].as_str().unwrap(),
            "dp record --source claude-code"
        );

        // Bobbin events added
        assert!(hooks["UserPromptSubmit"].is_array());
        assert!(hooks["SessionStart"].is_array());
        assert!(hooks["PostToolUse"].is_array());
    }

    #[test]
    fn test_merge_hooks_preserves_non_hook_settings() {
        // Top-level keys like statusLine must survive
        let mut settings = json!({
            "statusLine": {
                "command": "bash ~/.claude/statusline-command.sh",
                "type": "command"
            },
            "permissions": {
                "allow": ["Bash(cargo *)"]
            }
        });

        merge_hooks(&mut settings);

        assert_eq!(
            settings["statusLine"]["command"].as_str().unwrap(),
            "bash ~/.claude/statusline-command.sh"
        );
        assert_eq!(
            settings["permissions"]["allow"][0].as_str().unwrap(),
            "Bash(cargo *)"
        );
    }

    #[test]
    fn test_merge_hooks_realistic_multi_tool_settings() {
        // Mirrors a real ~/.claude/settings.json with Gas Town + dp hooks
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt mail check --inject" }
                        ],
                        "matcher": ""
                    }
                ],
                "SessionStart": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt prime --hook" }
                        ],
                        "matcher": ""
                    }
                ],
                "PreCompact": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt prime --hook" }
                        ],
                        "matcher": ""
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt costs record" }
                        ],
                        "matcher": ""
                    }
                ]
            },
            "statusLine": {
                "command": "bash ~/.claude/statusline-command.sh",
                "type": "command"
            }
        });

        merge_hooks(&mut settings);

        let hooks = &settings["hooks"];

        // Gas Town hooks in shared events preserved alongside bobbin
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2);
        assert_eq!(ups[0]["hooks"][0]["command"].as_str().unwrap(), "gt mail check --inject");
        assert_eq!(ups[1]["hooks"][0]["command"].as_str().unwrap(), "bobbin hook inject-context || true");

        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 2);
        assert_eq!(ss[0]["hooks"][0]["command"].as_str().unwrap(), "gt prime --hook");
        assert_eq!(ss[1]["hooks"][0]["command"].as_str().unwrap(), "bobbin hook session-context || true");

        // Events bobbin doesn't touch are untouched
        assert_eq!(hooks["PreCompact"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["Stop"].as_array().unwrap().len(), 1);

        // Non-hook settings preserved
        assert!(settings["statusLine"].is_object());
    }

    #[test]
    fn test_merge_hooks_idempotent_with_other_tools() {
        // Merge twice with non-bobbin hooks — should not duplicate anything
        let mut settings = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            { "type": "command", "command": "gt mail check --inject" }
                        ]
                    }
                ]
            }
        });

        merge_hooks(&mut settings);
        merge_hooks(&mut settings);

        let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 2, "gt hook + 1 bobbin hook, no duplicates");

        let ss = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1, "Only 1 bobbin SessionStart hook");
    }

    #[test]
    fn test_bobbin_hook_entries_structure() {
        let entries = bobbin_hook_entries();
        let hooks = entries.get("hooks").unwrap();

        // UserPromptSubmit
        let ups = hooks["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0]["hooks"][0]["type"].as_str().unwrap(), "command");
        assert_eq!(ups[0]["hooks"][0]["timeout"].as_i64().unwrap(), 10);

        // SessionStart
        let ss = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1);
        assert_eq!(ss[0]["matcher"].as_str().unwrap(), "compact");

        // PostToolUse
        let ptu = hooks["PostToolUse"].as_array().unwrap();
        assert_eq!(ptu.len(), 1);
        assert_eq!(ptu[0]["matcher"].as_str().unwrap(), "Write|Edit|Bash|Grep|Glob|Read");
        assert_eq!(
            ptu[0]["hooks"][0]["command"].as_str().unwrap(),
            "bobbin hook post-tool-use || true"
        );
        assert_eq!(ptu[0]["hooks"][0]["timeout"].as_i64().unwrap(), 10);

        // PostToolUseFailure
        let ptuf = hooks["PostToolUseFailure"].as_array().unwrap();
        assert_eq!(ptuf.len(), 1);
        assert_eq!(
            ptuf[0]["hooks"][0]["command"].as_str().unwrap(),
            "bobbin hook post-tool-use-failure || true"
        );
        assert_eq!(ptuf[0]["hooks"][0]["timeout"].as_i64().unwrap(), 10);
    }

    #[test]
    fn test_post_tool_use_input_deserialization() {
        let json = r#"{"session_id":"abc","tool_name":"Write","tool_input":{"file_path":"/tmp/test.rs","content":"fn main() {}"},"cwd":"/home/user/project","hook_event_name":"PostToolUse"}"#;
        let input: PostToolUseInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Write");
        assert_eq!(input.session_id, "abc");
        assert_eq!(input.cwd, "/home/user/project");
        assert_eq!(
            input.tool_input["file_path"].as_str().unwrap(),
            "/tmp/test.rs"
        );
    }

    #[test]
    fn test_post_tool_use_failure_input_deserialization() {
        let json = r#"{"session_id":"abc","tool_name":"Bash","tool_input":{"command":"cargo test"},"error":"Command exited with non-zero status code 1","cwd":"/home/user/project","hook_event_name":"PostToolUseFailure"}"#;
        let input: PostToolUseFailureInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.error, "Command exited with non-zero status code 1");
        assert_eq!(input.cwd, "/home/user/project");
        assert_eq!(
            input.tool_input["command"].as_str().unwrap(),
            "cargo test"
        );
    }

    #[test]
    fn test_post_tool_use_input_defaults() {
        // Minimal input - all fields should default gracefully
        let json = r#"{}"#;
        let input: PostToolUseInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "");
        assert_eq!(input.cwd, "");
        assert_eq!(input.session_id, "");
        assert!(input.tool_input.is_null());
    }

    #[test]
    fn test_post_tool_use_failure_input_defaults() {
        let json = r#"{}"#;
        let input: PostToolUseFailureInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "");
        assert_eq!(input.error, "");
        assert_eq!(input.cwd, "");
    }

    #[test]
    fn test_git_hook_section_has_markers() {
        assert!(GIT_HOOK_SECTION.contains(GIT_HOOK_START_MARKER));
        assert!(GIT_HOOK_SECTION.contains(GIT_HOOK_END_MARKER));
        assert!(GIT_HOOK_SECTION.contains("bobbin index --quiet"));
    }

    // --- Session dedup tests ---

    #[test]
    fn test_hook_state_serde_roundtrip() {
        let mut chunk_freqs = HashMap::new();
        chunk_freqs.insert(
            "src/foo.rs:10:50".to_string(),
            ChunkFrequency {
                count: 12,
                file: "src/foo.rs".to_string(),
                name: Some("InjectContextArgs".to_string()),
            },
        );
        let mut file_freqs = HashMap::new();
        file_freqs.insert("src/foo.rs".to_string(), 15);

        let state = HookState {
            last_session_id: "a1b2c3d4e5f6a7b8".to_string(),
            last_injected_chunks: vec!["src/foo.rs:10:50".to_string()],
            last_injection_time: "2026-02-08T10:30:00Z".to_string(),
            injection_count: 47,
            chunk_frequencies: chunk_freqs,
            file_frequencies: file_freqs,
            hot_topics_generated_at: 40,
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let parsed: HookState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.last_session_id, "a1b2c3d4e5f6a7b8");
        assert_eq!(parsed.injection_count, 47);
        assert_eq!(parsed.chunk_frequencies["src/foo.rs:10:50"].count, 12);
        assert_eq!(parsed.file_frequencies["src/foo.rs"], 15);
        assert_eq!(parsed.hot_topics_generated_at, 40);
    }

    #[test]
    fn test_hook_state_default() {
        let state = HookState::default();
        assert!(state.last_session_id.is_empty());
        assert!(state.last_injected_chunks.is_empty());
        assert_eq!(state.injection_count, 0);
        assert!(state.chunk_frequencies.is_empty());
        assert!(state.file_frequencies.is_empty());
    }

    #[test]
    fn test_hook_state_deserialize_corrupt_falls_back() {
        let corrupt = "{ not valid json at all }}}";
        let state: HookState = serde_json::from_str(corrupt).unwrap_or_default();
        assert!(state.last_session_id.is_empty());
        assert_eq!(state.injection_count, 0);
    }

    #[test]
    fn test_hook_state_deserialize_partial_fields() {
        // Only some fields present — rest should default
        let json = r#"{"last_session_id": "abc", "injection_count": 5}"#;
        let state: HookState = serde_json::from_str(json).unwrap();
        assert_eq!(state.last_session_id, "abc");
        assert_eq!(state.injection_count, 5);
        assert!(state.chunk_frequencies.is_empty());
        assert!(state.file_frequencies.is_empty());
    }

    #[test]
    fn test_load_save_hook_state() {
        let tmp = tempfile::tempdir().unwrap();
        let bobbin_dir = tmp.path().join(".bobbin");
        std::fs::create_dir_all(&bobbin_dir).unwrap();

        // Load from nonexistent file returns default
        let state = load_hook_state(tmp.path());
        assert!(state.last_session_id.is_empty());

        // Save and reload
        let mut state = HookState::default();
        state.last_session_id = "test123".to_string();
        state.injection_count = 3;
        save_hook_state(tmp.path(), &state);

        let loaded = load_hook_state(tmp.path());
        assert_eq!(loaded.last_session_id, "test123");
        assert_eq!(loaded.injection_count, 3);
    }

    #[test]
    fn test_compute_session_id_deterministic() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: vec![
                    ContextChunk {
                        name: Some("fn_a".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 10,
                        end_line: 20,
                        score: 0.9,
                        match_type: None,
                        content: None,
                    },
                    ContextChunk {
                        name: Some("fn_b".to_string()),
                        chunk_type: ChunkType::Function,
                        start_line: 30,
                        end_line: 40,
                        score: 0.8,
                        match_type: None,
                        content: None,
                    },
                ],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        let id1 = compute_session_id(&bundle, 0.5);
        let id2 = compute_session_id(&bundle, 0.5);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16); // 16 hex chars
    }

    #[test]
    fn test_compute_session_id_changes_with_different_chunks() {
        let make_bundle = |start: u32| ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: vec![ContextChunk {
                    name: None,
                    chunk_type: ChunkType::Function,
                    start_line: start,
                    end_line: start + 10,
                    score: 0.9,
                    match_type: None,
                    content: None,
                }],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 5,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 1,
                direct_hits: 1,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        let id1 = compute_session_id(&make_bundle(10), 0.0);
        let id2 = compute_session_id(&make_bundle(50), 0.0);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_compute_session_id_filters_by_threshold() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: vec![
                    ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Function,
                        start_line: 1,
                        end_line: 10,
                        score: 0.9,
                        match_type: None,
                        content: None,
                    },
                    ContextChunk {
                        name: None,
                        chunk_type: ChunkType::Function,
                        start_line: 20,
                        end_line: 30,
                        score: 0.3, // Below threshold
                        match_type: None,
                        content: None,
                    },
                ],
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 10,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 2,
                direct_hits: 2,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        // With threshold 0.5, low-score chunk is excluded
        let id_high = compute_session_id(&bundle, 0.5);
        // With threshold 0.0, both chunks included
        let id_low = compute_session_id(&bundle, 0.0);
        assert_ne!(id_high, id_low);
    }

    #[test]
    fn test_compute_session_id_empty_bundle() {
        let bundle = ContextBundle {
            query: "test".to_string(),
            files: vec![],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 0,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 0,
                total_chunks: 0,
                direct_hits: 0,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.0,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        let id = compute_session_id(&bundle, 0.0);
        assert_eq!(id.len(), 16);
    }

    #[test]
    fn test_compute_session_id_top_10_limit() {
        // Create 15 chunks — only first 10 (sorted alphabetically by key) should matter
        let chunks: Vec<ContextChunk> = (0..15)
            .map(|i| ContextChunk {
                name: None,
                chunk_type: ChunkType::Function,
                start_line: i * 10,
                end_line: i * 10 + 5,
                score: 0.9,
                match_type: None,
                content: None,
            })
            .collect();

        let bundle_all = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: chunks.clone(),
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 50,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 15,
                direct_hits: 15,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        // Build a bundle with the top-10 keys (alphabetically sorted) from all 15
        let mut all_keys: Vec<String> = chunks
            .iter()
            .map(|c| format!("src/a.rs:{}:{}", c.start_line, c.end_line))
            .collect();
        all_keys.sort();
        let top_10_keys: HashSet<String> = all_keys.into_iter().take(10).collect();

        let top_10_chunks: Vec<ContextChunk> = chunks
            .iter()
            .filter(|c| {
                let key = format!("src/a.rs:{}:{}", c.start_line, c.end_line);
                top_10_keys.contains(&key)
            })
            .cloned()
            .collect();

        let bundle_ten = ContextBundle {
            query: "test".to_string(),
            files: vec![ContextFile {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                relevance: FileRelevance::Direct,
                category: classify_file("src/a.rs"),
                score: 0.9,
                coupled_to: vec![],
                repo: None,
                chunks: top_10_chunks,
            }],
            budget: BudgetInfo {
                max_lines: 150,
                used_lines: 30,
                pinned_lines: 0,
            },
            summary: ContextSummary {
                total_files: 1,
                total_chunks: 10,
                direct_hits: 10,
                coupled_additions: 0,
                bridged_additions: 0,
                source_files: 0,
                doc_files: 0,
                top_semantic_score: 0.9,
                pinned_chunks: 0,
                knowledge_additions: 0,
            },
        };

        let id_all = compute_session_id(&bundle_all, 0.0);
        let id_ten = compute_session_id(&bundle_ten, 0.0);
        assert_eq!(id_all, id_ten, "Top-10 truncation should produce same ID");
    }

    // --- Session ledger (reducing) tests ---

    #[test]
    fn test_session_ledger_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = SessionLedger::load(tmp.path(), "test-session-1");
        assert_eq!(ledger.len(), 0);
        assert_eq!(ledger.turn, 0);
        assert!(!ledger.contains("src/foo.rs:10:20"));
    }

    #[test]
    fn test_session_ledger_record_and_query() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ledger = SessionLedger::load(tmp.path(), "test-session-2");

        let keys = vec![
            "src/foo.rs:10:20".to_string(),
            "src/bar.rs:5:15".to_string(),
        ];
        ledger.record(&keys, "inj-abc123");

        assert!(ledger.contains("src/foo.rs:10:20"));
        assert!(ledger.contains("src/bar.rs:5:15"));
        assert!(!ledger.contains("src/baz.rs:1:10"));
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger.turn, 1);
    }

    #[test]
    fn test_session_ledger_persistence() {
        let tmp = tempfile::tempdir().unwrap();

        // Record in one ledger instance
        {
            let mut ledger = SessionLedger::load(tmp.path(), "test-session-3");
            ledger.record(&["src/a.rs:1:10".to_string()], "inj-001");
            assert_eq!(ledger.turn, 1);
        }

        // Reload — entries should persist
        {
            let ledger = SessionLedger::load(tmp.path(), "test-session-3");
            assert!(ledger.contains("src/a.rs:1:10"));
            assert_eq!(ledger.len(), 1);
            assert_eq!(ledger.turn, 1);
        }
    }

    #[test]
    fn test_session_ledger_multi_turn() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ledger = SessionLedger::load(tmp.path(), "test-session-4");

        // Turn 1
        ledger.record(&["src/a.rs:1:10".to_string(), "src/b.rs:1:10".to_string()], "inj-001");
        assert_eq!(ledger.turn, 1);
        assert_eq!(ledger.len(), 2);

        // Turn 2 — new chunks plus overlap
        ledger.record(&["src/c.rs:1:10".to_string()], "inj-002");
        assert_eq!(ledger.turn, 2);
        assert_eq!(ledger.len(), 3);

        // All three chunks present
        assert!(ledger.contains("src/a.rs:1:10"));
        assert!(ledger.contains("src/b.rs:1:10"));
        assert!(ledger.contains("src/c.rs:1:10"));
    }

    #[test]
    fn test_session_ledger_clear() {
        let tmp = tempfile::tempdir().unwrap();

        // Record some data
        {
            let mut ledger = SessionLedger::load(tmp.path(), "test-session-5");
            ledger.record(&["src/a.rs:1:10".to_string()], "inj-001");
        }

        // Clear it
        SessionLedger::clear(tmp.path(), "test-session-5");

        // Reload — should be empty
        {
            let ledger = SessionLedger::load(tmp.path(), "test-session-5");
            assert_eq!(ledger.len(), 0);
            assert_eq!(ledger.turn, 0);
        }
    }

    #[test]
    fn test_session_ledger_empty_session_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ledger = SessionLedger::load(tmp.path(), "");
        assert!(ledger.path.is_none());

        // Should work in-memory without crashing
        ledger.record(&["src/a.rs:1:10".to_string()], "inj-001");
        assert!(ledger.contains("src/a.rs:1:10"));
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn test_chunk_key_format() {
        assert_eq!(chunk_key("src/foo.rs", 10, 20), "src/foo.rs:10:20");
        assert_eq!(chunk_key("/var/lib/repos/x/main.go", 1, 100), "/var/lib/repos/x/main.go:1:100");
    }

    // --- Hot topics tests ---

    #[test]
    fn test_generate_hot_topics_empty_state() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let state = HookState::default();
        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Hot Topics (auto-generated by bobbin)"));
        assert!(content.contains("Based on 0 context injections."));
        assert!(content.contains("No injection data yet."));
        assert!(content.contains("## Frequently Referenced Code"));
        assert!(content.contains("## Most Referenced Files"));
    }

    #[test]
    fn test_generate_hot_topics_with_data() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut chunk_freqs = HashMap::new();
        chunk_freqs.insert(
            "src/cli/hook.rs:10:50".to_string(),
            ChunkFrequency {
                count: 12,
                file: "src/cli/hook.rs".to_string(),
                name: Some("InjectContextArgs".to_string()),
            },
        );
        chunk_freqs.insert(
            "src/config.rs:20:40".to_string(),
            ChunkFrequency {
                count: 9,
                file: "src/config.rs".to_string(),
                name: Some("HooksConfig".to_string()),
            },
        );
        chunk_freqs.insert(
            "src/search/context.rs:5:30".to_string(),
            ChunkFrequency {
                count: 7,
                file: "src/search/context.rs".to_string(),
                name: None,
            },
        );

        let mut file_freqs = HashMap::new();
        file_freqs.insert("src/cli/hook.rs".to_string(), 15);
        file_freqs.insert("src/config.rs".to_string(), 12);
        file_freqs.insert("src/search/context.rs".to_string(), 9);

        let state = HookState {
            last_session_id: "abc123".to_string(),
            last_injected_chunks: vec![],
            last_injection_time: "2026-02-08T10:30:00Z".to_string(),
            injection_count: 47,
            chunk_frequencies: chunk_freqs,
            file_frequencies: file_freqs,
            hot_topics_generated_at: 40,
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("Based on 47 context injections."));
        assert!(content.contains("2026-02-08 10:30 UTC"));

        // Chunks should be ranked by count descending
        let hook_pos = content.find("InjectContextArgs").unwrap();
        let config_pos = content.find("HooksConfig").unwrap();
        assert!(hook_pos < config_pos, "Higher-count chunk should appear first");

        // File table present and ranked
        assert!(content.contains("| src/cli/hook.rs | 15 |"));
        assert!(content.contains("| src/config.rs | 12 |"));

        // Symbol-less chunk shows dash
        assert!(content.contains("| - |"));
    }

    #[test]
    fn test_generate_hot_topics_truncates_chunks_to_20() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut chunk_freqs = HashMap::new();
        for i in 0..30 {
            chunk_freqs.insert(
                format!("src/file{}.rs:1:10", i),
                ChunkFrequency {
                    count: 30 - i,
                    file: format!("src/file{}.rs", i),
                    name: Some(format!("fn_{}", i)),
                },
            );
        }

        let state = HookState {
            injection_count: 100,
            chunk_frequencies: chunk_freqs,
            ..Default::default()
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        // Count chunk table rows (lines starting with "| <digit>")
        let chunk_section = content
            .split("## Frequently Referenced Code")
            .nth(1)
            .unwrap()
            .split("## Most Referenced Files")
            .next()
            .unwrap();
        let rank_rows: Vec<&str> = chunk_section
            .lines()
            .filter(|l| l.starts_with("| ") && l.chars().nth(2).map_or(false, |c| c.is_ascii_digit()))
            .collect();
        assert_eq!(rank_rows.len(), 20);
    }

    #[test]
    fn test_generate_hot_topics_truncates_files_to_10() {
        let tmp = tempfile::tempdir().unwrap();
        let output_path = tmp.path().join("hot-topics.md");

        let mut file_freqs = HashMap::new();
        for i in 0..15 {
            file_freqs.insert(format!("src/file{}.rs", i), 15 - i as u64);
        }

        let state = HookState {
            injection_count: 50,
            file_frequencies: file_freqs,
            ..Default::default()
        };

        generate_hot_topics(&state, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        // Count table rows in the file section
        let file_section = content.split("## Most Referenced Files").nth(1).unwrap();
        let row_count = file_section.lines().filter(|l| l.starts_with("| src/")).count();
        assert_eq!(row_count, 10);
    }

    #[test]
    fn test_extract_grep_pattern() {
        // Basic grep
        assert_eq!(
            extract_search_query_from_bash("grep -r \"Stmt::Import\" src/"),
            Some("Stmt::Import".to_string())
        );

        // rg with type flag
        assert_eq!(
            extract_search_query_from_bash("rg \"fn main\" --type rust"),
            Some("fn main".to_string())
        );

        // grep with -i flag
        assert_eq!(
            extract_search_query_from_bash("grep -ri \"error handling\" ."),
            Some("error handling".to_string())
        );

        // rg with single quotes
        assert_eq!(
            extract_search_query_from_bash("rg 'impl Display' src/"),
            Some("impl Display".to_string())
        );

        // git grep
        assert_eq!(
            extract_search_query_from_bash("git grep \"TODO\" -- '*.rs'"),
            Some("TODO".to_string())
        );

        // Not a grep command
        assert_eq!(
            extract_search_query_from_bash("cargo build --release"),
            None
        );

        // grep with -e flag (pattern follows -e)
        assert_eq!(
            extract_search_query_from_bash("grep -r -e \"pattern\" src/"),
            Some("pattern".to_string())
        );
    }

    #[test]
    fn test_extract_find_pattern() {
        // find with -name
        assert_eq!(
            extract_search_query_from_bash("find . -name \"*.test.rs\""),
            Some("test.rs".to_string())
        );

        // find with -iname
        assert_eq!(
            extract_search_query_from_bash("find src/ -iname \"*.py\""),
            Some("py".to_string())
        );

        // find without -name
        assert_eq!(
            extract_search_query_from_bash("find . -type f"),
            None
        );
    }

    #[test]
    fn test_clean_regex_for_search() {
        assert_eq!(clean_regex_for_search("fn\\s+main"), "fn main");
        assert_eq!(clean_regex_for_search("impl.*Display"), "impl Display");
        assert_eq!(clean_regex_for_search("^use\\b"), "use");
        assert_eq!(clean_regex_for_search("Stmt::Import"), "Stmt::Import");
    }

    #[test]
    fn test_is_meaningful_search_query() {
        // Too short
        assert!(!is_meaningful_search_query(""));
        assert!(!is_meaningful_search_query("fn"));
        assert!(!is_meaningful_search_query("rs"));

        // Single noise words (language keywords, file extensions)
        assert!(!is_meaningful_search_query("let"));
        assert!(!is_meaningful_search_query("import"));
        assert!(!is_meaningful_search_query("toml"));
        assert!(!is_meaningful_search_query("json"));

        // Meaningful queries
        assert!(is_meaningful_search_query("PostToolUse"));
        assert!(is_meaningful_search_query("context assembler"));
        assert!(is_meaningful_search_query("fn main")); // multi-word is fine
        assert!(is_meaningful_search_query("search query"));
        assert!(is_meaningful_search_query("ContextConfig"));
    }

    #[test]
    fn test_is_source_code_file() {
        // Source code files — refs are useful
        assert!(is_source_code_file("src/main.rs"));
        assert!(is_source_code_file("/home/user/project/handler.go"));
        assert!(is_source_code_file("app.py"));
        assert!(is_source_code_file("components/Button.tsx"));

        // Non-source files — refs not useful
        assert!(!is_source_code_file("README.md"));
        assert!(!is_source_code_file("config.toml"));
        assert!(!is_source_code_file("package.json"));
        assert!(!is_source_code_file("styles.css"));
        assert!(!is_source_code_file("Makefile"));
        assert!(!is_source_code_file("data.yaml"));
    }

    #[test]
    fn test_strip_system_tags() {
        // System reminder blocks
        assert_eq!(
            strip_system_tags("Hello <system-reminder>noise</system-reminder> world"),
            "Hello  world"
        );
        // Task notification blocks
        assert_eq!(
            strip_system_tags("Query <task-notification>task-id: abc</task-notification> here"),
            "Query  here"
        );
        // Both types together
        let input = "<system-reminder>sys</system-reminder>real content<task-notification>task</task-notification>";
        assert_eq!(strip_system_tags(input), "real content");
        // No tags
        assert_eq!(strip_system_tags("plain text"), "plain text");
    }

    #[test]
    fn test_is_automated_message() {
        // Patrol nudges
        assert!(is_automated_message("Auto-patrol: pick up aegis-abc123 (Some task). Run: bd show aegis-abc123"));
        assert!(is_automated_message("PATROL LOOP — you must keep working until context is below 20%."));
        assert!(is_automated_message("RANGER PATROL: You are a ranger. Patrol your domain."));
        assert!(is_automated_message("PATROL: Run gt hook, gt mail inbox, bd ready."));

        // Reactor alerts
        assert!(is_automated_message("[reactor] ⚠️ ESCALATION: E2ESmokeTestFailing — luvu | Paging: aegis/crew/wu"));
        assert!(is_automated_message("[reactor] 🟠 P1 bead: aegis-sc86f0 Skills Framework Phase 1"));
        assert!(is_automated_message("[reactor] 🟠 P0 bead: aegis-thmbt2 Claude token expires"));

        // Repeated work nudges
        assert!(is_automated_message("WORK: You are stryder (Bobbin Ranger). Check gt hook and gt mail inbox. Keep working until context below 25%, then /handoff."));

        // Startup/handoff messages
        assert!(is_automated_message("╔══════╗\n║  ✅ HANDOFF COMPLETE - You are the NEW session  ║\n╚══════╝\nYour predecessor handed off to you."));
        assert!(is_automated_message("**STARTUP PROTOCOL**: Please:\n1. Run `gt hook` — What's hooked?"));

        // Marshal/dog checks
        assert!(is_automated_message("[from dog] Marshal check: You appear idle (7+ days no commits). Check bd ready."));

        // Queued nudge wrappers
        assert!(is_automated_message("QUEUED NUDGE (1 message(s)):\n\n  [from dog] check status\n\nThis is a background notification. Continue current work."));

        // Agent role announcements
        assert!(is_automated_message("aegis Crew ian, checking in."));
        assert!(is_automated_message("\naegis Crew mel, checking in.\n"));

        // System reminder blocks
        assert!(is_automated_message("<system-reminder>\nUserPromptSubmit hook success\n</system-reminder>"));
        assert!(is_automated_message("[GAS TOWN] crew ian (rig: aegis) <- self"));

        // Handoff mail directives
        assert!(is_automated_message("Check your hook and mail, then act on the hook if present:\n1. `gt hook`"));

        // Normal messages should NOT be filtered
        assert!(!is_automated_message("Fix the bug in bobbin search"));
        assert!(!is_automated_message("How do I deploy bobbin to kota?"));
        assert!(!is_automated_message("bd show aegis-abc123"));
        assert!(!is_automated_message("Run the tests and check for failures"));
        assert!(!is_automated_message("")); // Empty string

        // Whitespace-trimmed patterns should still match
        assert!(is_automated_message("  \n<system-reminder>\nhook output\n</system-reminder>"));
        assert!(is_automated_message("\n[GAS TOWN] crew ian (rig: aegis) <- self"));
    }

    #[test]
    fn test_is_bead_command() {
        // Bead commands that should be skipped
        assert!(is_bead_command("remove bo-qq5h"));
        assert!(is_bead_command("show aegis-abc123"));
        assert!(is_bead_command("close gt-xyz"));
        assert!(is_bead_command("hook gt-h8x"));
        assert!(is_bead_command("bd show aegis-ky3wc9"));
        assert!(is_bead_command("unhook hq-abc"));
        assert!(is_bead_command("aegis-mlpgac"));

        // Should NOT be skipped (not bead commands)
        assert!(!is_bead_command("Fix the bug in bobbin search"));
        assert!(!is_bead_command("How do I deploy bobbin to kota?"));
        assert!(!is_bead_command("Run the tests and check for failures"));
        assert!(!is_bead_command("")); // Empty string
        assert!(!is_bead_command("what is the architecture of the system and how does deployment work across all rigs"));
        // Too short suffix (< 3 chars)
        assert!(!is_bead_command("show x-ab"));
        // Not lowercase prefix
        assert!(!is_bead_command("show ABC-def123"));
    }

    #[test]
    fn test_session_ledger_injected_files() {
        let mut ledger = SessionLedger {
            entries: HashSet::new(),
            turn: 0,
            path: None,
        };
        // Simulate chunk keys
        ledger.entries.insert("src/main.rs:1:10".to_string());
        ledger.entries.insert("src/main.rs:20:30".to_string());
        ledger.entries.insert("src/config.rs:5:15".to_string());
        ledger.entries.insert("tests/test_auth.rs:1:50".to_string());

        let files = ledger.injected_files();
        assert_eq!(files.len(), 3, "should have 3 unique files");
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"src/config.rs".to_string()));
        assert!(files.contains(&"tests/test_auth.rs".to_string()));
    }

    #[test]
    fn test_session_ledger_injected_files_empty() {
        let ledger = SessionLedger {
            entries: HashSet::new(),
            turn: 0,
            path: None,
        };
        assert!(ledger.injected_files().is_empty());
    }

    #[test]
    fn test_prompt_history_trajectory_empty() {
        let history = PromptHistory {
            entries: Vec::new(),
            path: None,
            max_entries: 5,
        };
        let query = history.build_trajectory_query("current prompt", 700);
        assert_eq!(query, "current prompt");
    }

    #[test]
    fn test_prompt_history_trajectory_with_history() {
        let history = PromptHistory {
            entries: vec![
                PromptEntry { prompt: "how does auth work".to_string(), timestamp: 100 },
                PromptEntry { prompt: "show me the middleware".to_string(), timestamp: 200 },
            ],
            path: None,
            max_entries: 5,
        };
        let query = history.build_trajectory_query("error handling in routes", 700);
        // Should contain history + current, separated by |
        assert!(query.contains("auth"));
        assert!(query.contains("middleware"));
        assert!(query.contains("error handling in routes"));
        assert!(query.contains(" | "));
    }

    #[test]
    fn test_prompt_history_trajectory_dedup_current() {
        let history = PromptHistory {
            entries: vec![
                PromptEntry { prompt: "same prompt".to_string(), timestamp: 100 },
            ],
            path: None,
            max_entries: 5,
        };
        let query = history.build_trajectory_query("same prompt", 700);
        // Should NOT duplicate the current prompt
        assert_eq!(query, "same prompt");
    }

    #[test]
    fn test_prompt_history_trajectory_respects_max_chars() {
        let history = PromptHistory {
            entries: vec![
                PromptEntry { prompt: "a".repeat(500), timestamp: 100 },
            ],
            path: None,
            max_entries: 5,
        };
        let query = history.build_trajectory_query("current", 100);
        // Total should not exceed max_chars
        assert!(query.len() <= 200, "query too long: {}", query.len());
    }

    #[test]
    fn test_prompt_history_record_and_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo_root = tmp.path();
        let session_dir = repo_root.join(".bobbin").join("session").join("test-sess");
        std::fs::create_dir_all(&session_dir).unwrap();

        let mut history = PromptHistory::load(repo_root, "test-sess", 3);
        assert!(history.entries.is_empty());

        history.record("first prompt");
        history.record("second prompt");
        history.record("third prompt");

        // Reload from disk
        let reloaded = PromptHistory::load(repo_root, "test-sess", 3);
        assert_eq!(reloaded.entries.len(), 3);
        assert_eq!(reloaded.entries[0].prompt, "first prompt");
        assert_eq!(reloaded.entries[2].prompt, "third prompt");

        // Add one more — should maintain max_entries=3
        let mut history2 = PromptHistory::load(repo_root, "test-sess", 3);
        history2.record("fourth prompt");
        assert_eq!(history2.entries.len(), 3);
        assert_eq!(history2.entries[0].prompt, "second prompt"); // first was trimmed
        assert_eq!(history2.entries[2].prompt, "fourth prompt");
    }
