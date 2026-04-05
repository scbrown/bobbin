use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};
use super::types::{PostToolUseInput, HookResponse, HookSpecificOutput, find_bobbin_root};
use super::util::{detect_repo_name, extract_search_query_from_bash, clean_regex_for_search, is_meaningful_search_query, is_source_code_file};
use super::{PostToolUseArgs, OutputConfig};

pub(super) async fn run_post_tool_use(_args: PostToolUseArgs, _output: OutputConfig) -> Result<()> {
    // Never block tool completion — any error exits silently
    match run_post_tool_use_inner(_args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin post-tool-use: {:#}", e);
            Ok(())
        }
    }
}


/// PostToolUse handler: Smart dispatch based on tool type.
/// - Edit/Write: hybrid search for related files (tests, snapshots, configs)
/// - Bash(grep/rg/find): semantic search for the same query (competitive response)
/// - Any tool: reaction rules from .bobbin/reactions.toml
/// Uses ContextAssembler with full config cascade (calibration + config.toml).
/// Fast because ensure_fts_index reuses persisted index.
async fn run_post_tool_use_inner(args: PostToolUseArgs) -> Result<()> {
    use crate::index::Embedder;
    use crate::reactions::{self, CompiledRule, DedupTracker, ReactionConfig, ToolEvent};
    use crate::search::context::{BridgeMode, ContentMode, ContextAssembler, ContextConfig};

    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON
    let input: PostToolUseInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    // 2. Dispatch based on tool type
    enum DispatchMode {
        EditRelated { file_path: String },
        SearchQuery { query: String, original_cmd: String },
        RefsOnly { file_path: String },
        ReactionsOnly, // Unknown tool — only reactions, no built-in dispatch
    }

    let mode = match input.tool_name.as_str() {
        "Edit" | "Write" => {
            let file_path = input
                .tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if file_path.is_empty() {
                DispatchMode::ReactionsOnly
            } else {
                DispatchMode::EditRelated { file_path }
            }
        }
        "Bash" => {
            let command = input
                .tool_input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match extract_search_query_from_bash(command) {
                Some(query) if is_meaningful_search_query(&query) => {
                    DispatchMode::SearchQuery {
                        query,
                        original_cmd: command.to_string(),
                    }
                }
                _ => DispatchMode::ReactionsOnly,
            }
        }
        "Grep" => {
            // Claude Code's built-in Grep tool
            let pattern = input
                .tool_input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if pattern.len() < 2 {
                DispatchMode::ReactionsOnly
            } else {
                let cleaned = clean_regex_for_search(pattern);
                if cleaned.is_empty() || !is_meaningful_search_query(&cleaned) {
                    DispatchMode::ReactionsOnly
                } else {
                    DispatchMode::SearchQuery {
                        query: cleaned,
                        original_cmd: format!("Grep: {}", pattern),
                    }
                }
            }
        }
        "Glob" => {
            let pattern = input
                .tool_input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if pattern.len() < 2 {
                DispatchMode::ReactionsOnly
            } else {
                // Strip glob wildcards for semantic search, keeping meaningful path segments
                let cleaned = pattern
                    .replace("**", " ")
                    .replace("*.", "")
                    .replace(".*", "")
                    .replace('*', " ")
                    .replace('/', " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if cleaned.len() < 2 || !is_meaningful_search_query(&cleaned) {
                    DispatchMode::ReactionsOnly
                } else {
                    DispatchMode::SearchQuery {
                        query: cleaned,
                        original_cmd: format!("Glob: {}", pattern),
                    }
                }
            }
        }
        "Read" => {
            let file_path = input
                .tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if file_path.is_empty() || !is_source_code_file(&file_path) {
                DispatchMode::ReactionsOnly
            } else {
                DispatchMode::RefsOnly { file_path }
            }
        }
        _ => DispatchMode::ReactionsOnly,
    };

    // 2b. Build ToolEvent for reaction matching
    let tool_event = ToolEvent {
        tool_name: input.tool_name.clone(),
        tool_input: input.tool_input.clone(),
    };

    // 3. Resolve bobbin root and config
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
    };

    let repo_root = match find_bobbin_root(&cwd) {
        Some(r) => r,
        None => return Ok(()), // Not a bobbin-indexed project
    };

    let config = Config::load(&Config::config_path(&repo_root)).unwrap_or_default();
    let budget = args.budget.unwrap_or(config.hooks.budget / 2); // Use half budget for post-tool

    let metrics_source = crate::metrics::resolve_source(
        None,
        if input.session_id.is_empty() { None } else { Some(&input.session_id) },
    );

    // 3b. Resolve role for reaction filtering
    let role = crate::access::RepoFilter::resolve_role(None);

    // 3b'. Load reaction rules (builtins + user overrides) and compile them
    let reaction_config = ReactionConfig::load_for_repo(&repo_root).with_builtins();
    let compiled_rules: Vec<CompiledRule> = reaction_config
        .reactions
        .into_iter()
        .filter_map(|r| {
            CompiledRule::compile(r).map_err(|e| {
                eprintln!("bobbin: skipping reaction rule: {}", e);
                e
            }).ok()
        })
        .collect();
    let has_reactions = !compiled_rules.is_empty();

    // 3c. Session dedup tracker for reactions
    let mut dedup = DedupTracker::load(&repo_root, &input.session_id);

    // For ReactionsOnly mode with no reaction rules, nothing to do
    if matches!(mode, DispatchMode::ReactionsOnly) && !has_reactions {
        return Ok(());
    }

    // 4. Determine query and context based on dispatch mode
    let (query, rel_path, is_edit_mode, is_refs_only, is_reactions_only) = match &mode {
        DispatchMode::EditRelated { file_path } => {
            let abs_path = if Path::new(file_path.as_str()).is_absolute() {
                PathBuf::from(file_path)
            } else {
                cwd.join(file_path)
            };
            let rel = abs_path
                .strip_prefix(&repo_root)
                .unwrap_or(abs_path.as_path())
                .to_string_lossy()
                .to_string();
            let q = format!("files related to {}", rel);
            (q, Some(rel), true, false, false)
        }
        DispatchMode::SearchQuery { query, .. } => {
            (query.clone(), None, false, false, false)
        }
        DispatchMode::RefsOnly { file_path } => {
            let abs_path = if Path::new(file_path.as_str()).is_absolute() {
                PathBuf::from(file_path)
            } else {
                cwd.join(file_path)
            };
            let rel = abs_path
                .strip_prefix(&repo_root)
                .unwrap_or(abs_path.as_path())
                .to_string_lossy()
                .to_string();
            ("".to_string(), Some(rel), false, true, false)
        }
        DispatchMode::ReactionsOnly => {
            ("".to_string(), None, false, false, true)
        }
    };

    // 5. Open stores
    let db_path = Config::db_path(&repo_root);
    let lance_path = Config::lance_path(&repo_root);

    // For RefsOnly (Read), we skip the search/related logic entirely
    let mut context = String::new();
    use std::fmt::Write;
    let mut lines_used: usize = 0;
    let mut coupled_count: usize = 0;
    let mut search_file_count: usize = 0;

    if !is_refs_only && !is_reactions_only {
        let model_dir = Config::model_cache_dir()?;

        // Try to open stores — failure skips builtin search but reactions still fire
        let builtin_result: Option<()> = 'builtin: {
            let vector_store = match VectorStore::open(&lance_path).await {
                Ok(vs) if vs.count().await.unwrap_or(0) > 0 => vs,
                _ => break 'builtin None,
            };

            let metadata_store = match MetadataStore::open(&db_path) {
                Ok(ms) => ms,
                Err(_) => break 'builtin None,
            };

            let embedder = match Embedder::from_config(&config.embedding, &model_dir) {
                Ok(e) => e,
                Err(_) => break 'builtin None,
            };

        // 6. Query coupled files (only for Edit mode — coupling is file-based)
        let coupled: Vec<(String, f32)> = if let Some(ref rp) = rel_path {
            let coupled_raw = metadata_store.get_coupling(rp, 5).unwrap_or_default();
            coupled_raw
                .iter()
                .filter(|c| c.score >= 0.1)
                .map(|c| {
                    let other = if c.file_a == *rp {
                        c.file_b.clone()
                    } else {
                        c.file_a.clone()
                    };
                    (other, c.score)
                })
                .collect()
        } else {
            vec![]
        };

        // 7. Hybrid search — uses calibrated config for search quality.
        let calibration = crate::cli::calibrate::load_calibration(&repo_root);
        let cal_sw = calibration.as_ref().map(|c| c.best_config.semantic_weight);
        let cal_dd = calibration.as_ref().map(|c| c.best_config.doc_demotion);
        let cal_rrf = calibration.as_ref().map(|c| c.best_config.rrf_k);
        let cal_hl = calibration.as_ref().and_then(|c| c.best_config.recency_half_life_days);
        let cal_rw = calibration.as_ref().and_then(|c| c.best_config.recency_weight);
        let cal_sl = calibration.as_ref().and_then(|c| c.best_config.search_limit);

        let context_config = ContextConfig {
            budget_lines: budget,
            depth: 0, // No recursive expansion for post-tool
            max_coupled: 0, // We handle coupling separately above
            coupling_threshold: 0.1,
            semantic_weight: cal_sw.unwrap_or(config.search.semantic_weight),
            content_mode: ContentMode::None, // File list only, no content
            search_limit: cal_sl.unwrap_or(10), // Smaller default for speed
            doc_demotion: cal_dd.unwrap_or(config.search.doc_demotion),
            recency_half_life_days: cal_hl.unwrap_or(config.search.recency_half_life_days),
            recency_weight: cal_rw.unwrap_or(config.search.recency_weight),
            rrf_k: cal_rrf.unwrap_or(config.search.rrf_k),
            bridge_mode: BridgeMode::Off, // No bridging for post-tool
            bridge_boost_factor: 0.0,
            extra_filter: None,
            tags_config: None,
            role: None,
            file_type_rules: config.file_types.clone(),
            repo_affinity: detect_repo_name(&cwd),
            repo_affinity_boost: config.hooks.repo_affinity_boost,
            max_bridged_files: 3,
            max_bridged_chunks_per_file: 2,
            repo_path_prefix: config.server.repo_path_prefix.clone(),
            ..ContextConfig::default()
        };

        let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
        if let Ok(git) = crate::index::git::GitAnalyzer::new(&repo_root) {
            assembler = assembler.with_git_analyzer(git);
        }

        let bundle = match assembler.assemble(&query, None).await {
            Ok(b) => b,
            Err(_) => {
                // Search failed — still report coupling if available
                if coupled.is_empty() {
                    return Ok(());
                }
                crate::search::context::ContextBundle {
                    query: query.clone(),
                    files: vec![],
                    budget: crate::search::context::BudgetInfo { max_lines: budget, used_lines: 0, pinned_lines: 0 },
                    summary: crate::search::context::ContextSummary {
                        total_files: 0, total_chunks: 0, direct_hits: 0,
                        coupled_additions: 0, bridged_additions: 0,
                        source_files: 0, doc_files: 0, top_semantic_score: 0.0,
                        pinned_chunks: 0, knowledge_additions: 0,
                    },
                }
            }
        };

        // Filter out the edited file itself and low-score results
        // For non-edit search modes, apply a stricter score threshold to reduce noise
        let min_score = if is_edit_mode { 0.0 } else { 0.005 };
        let search_files: Vec<_> = bundle
            .files
            .iter()
            .filter(|f| {
                // Score gate: skip low-relevance results (especially for search tools)
                if f.score < min_score {
                    return false;
                }
                // Skip the edited file itself (for Edit mode)
                if let Some(ref rp) = rel_path {
                    let f_rel = Path::new(&f.path)
                        .strip_prefix(&repo_root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| f.path.clone());
                    f_rel != *rp
                } else {
                    true
                }
            })
            .collect();

        coupled_count = coupled.len();
        search_file_count = search_files.len();

        // 8. Format output — different framing for Edit vs Search dispatch
        if is_edit_mode {
            let rp = rel_path.as_deref().unwrap_or("unknown");
            let _ = writeln!(context, "## Related Files: {}", rp);
            let _ = writeln!(context, "You just edited this file. Consider reviewing these related files:\n");
            lines_used += 3;

            if !coupled.is_empty() {
                let _ = writeln!(context, "**Co-changing files** (from git history):");
                lines_used += 1;
                for (coupled_file, score) in &coupled {
                    if lines_used >= budget {
                        break;
                    }
                    let _ = writeln!(context, "- `{}` (coupling: {:.2})", coupled_file, score);
                    lines_used += 1;
                }
                let _ = writeln!(context);
                lines_used += 1;
            }

            if !search_files.is_empty() {
                let _ = writeln!(context, "**Semantically related** (from bobbin search):");
                lines_used += 1;
            }
        } else if !search_files.is_empty() {
            // Only show search results header if we have results above the score gate
            let original_cmd = match &mode {
                DispatchMode::SearchQuery { original_cmd, .. } => original_cmd.as_str(),
                _ => "search",
            };
            let _ = writeln!(context, "## Bobbin Semantic Matches");
            let _ = writeln!(context, "Your search (`{}`) also matched these files semantically:\n", original_cmd);
            lines_used += 3;
        }

        if !search_files.is_empty() {
            for f in &search_files {
                if lines_used >= budget {
                    break;
                }
                let f_rel = Path::new(&f.path)
                    .strip_prefix(&repo_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| f.path.clone());
                let _ = writeln!(context, "- `{}`", f_rel);
                lines_used += 1;
            }
        }

            Some(()) // end of labeled block
        }; // end 'builtin block
        let _ = builtin_result; // suppress unused warning
    }

    // 9. Symbol refs lookup (Edit + Read modes) — callers AND callees
    let mut refs_count: usize = 0;
    let mut callees_count: usize = 0;
    if (is_edit_mode || is_refs_only) && lines_used < budget {
        if let Some(ref rp) = rel_path {
            let refs_vs_result = VectorStore::open(&lance_path).await;
            // Scope the refs analysis in a block — if store open fails, skip refs
            // but continue to reactions below
            if let Ok(mut refs_vs) = refs_vs_result {

            use crate::analysis::refs::RefAnalyzer;

            // list_symbols needs the path as stored in the index (absolute)
            let abs_file = repo_root.join(rp);
            let abs_file_str = abs_file.to_string_lossy().to_string();

            // Pre-fetch file chunks before creating the analyzer (to avoid borrow conflicts)
            let file_chunks = refs_vs
                .get_chunks_for_file(&abs_file_str, None)
                .await
                .unwrap_or_default();

            let mut analyzer = RefAnalyzer::new(&mut refs_vs);

            // List symbols in the file
            let file_symbols = analyzer.list_symbols(&abs_file_str, None).await.unwrap_or_else(|_| {
                crate::analysis::refs::FileSymbols {
                    path: abs_file_str.clone(),
                    symbols: vec![],
                }
            });

            if !file_symbols.symbols.is_empty() {
                // Limit to top 3 symbols (by line order — most prominent definitions first)
                let symbols_to_check: Vec<_> = file_symbols.symbols.iter().take(3).collect();

                // 9a. Callers: for each symbol, find where it's used (in other files)
                let mut symbol_refs: Vec<(String, Vec<String>)> = Vec::new();
                for sym in &symbols_to_check {
                    let refs = analyzer
                        .find_refs(&sym.name, None, 10, None)
                        .await
                        .unwrap_or_else(|_| crate::analysis::refs::SymbolRefs {
                            definition: None,
                            usages: vec![],
                        });

                    // Collect unique files where this symbol is used (excluding the file itself)
                    // Usage file_paths are absolute — convert to relative for display and comparison
                    let mut usage_files: Vec<String> = refs
                        .usages
                        .iter()
                        .map(|u| {
                            Path::new(&u.file_path)
                                .strip_prefix(&repo_root)
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|_| u.file_path.clone())
                        })
                        .filter(|f| f != rp)
                        .collect();
                    usage_files.dedup();
                    usage_files.truncate(5);

                    if !usage_files.is_empty() {
                        symbol_refs.push((sym.name.clone(), usage_files));
                    }
                }

                if !symbol_refs.is_empty() {
                    refs_count = symbol_refs.len();
                    if lines_used > 0 {
                        let _ = writeln!(context);
                        lines_used += 1;
                    }

                    if is_refs_only {
                        let _ = writeln!(context, "## Symbol References: {}", rp);
                        let _ = writeln!(context, "Symbols defined in this file are used in:\n");
                        lines_used += 3;
                    } else {
                        let _ = writeln!(context, "**Symbol references** (where symbols from this file are used):");
                        lines_used += 1;
                    }

                    for (sym_name, usage_files) in &symbol_refs {
                        if lines_used >= budget {
                            break;
                        }
                        let _ = writeln!(context, "- `{}` → {}", sym_name,
                            usage_files.iter().map(|f| format!("`{}`", f)).collect::<Vec<_>>().join(", "));
                        lines_used += 1;
                    }
                }

                // 9b. Callees: for each symbol, find what functions it calls
                if lines_used < budget {
                    let mut symbol_callees: Vec<(String, Vec<(String, String)>)> = Vec::new();
                    for sym in &symbols_to_check {
                        // Find the chunk content for this symbol
                        let chunk = file_chunks.iter().find(|c| {
                            c.name.as_deref() == Some(&sym.name)
                        });
                        if let Some(chunk) = chunk {
                            let callees = analyzer
                                .find_callees(&chunk.content, Some(&sym.name), 5, None)
                                .await
                                .unwrap_or_default();

                            let callee_info: Vec<(String, String)> = callees
                                .into_iter()
                                .filter_map(|c| {
                                    let def = c.definition?;
                                    let rel_file = Path::new(&def.file_path)
                                        .strip_prefix(&repo_root)
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|_| def.file_path);
                                    Some((c.name, rel_file))
                                })
                                .collect();

                            if !callee_info.is_empty() {
                                symbol_callees.push((sym.name.clone(), callee_info));
                            }
                        }
                    }

                    if !symbol_callees.is_empty() {
                        callees_count = symbol_callees.len();
                        if lines_used > 0 {
                            let _ = writeln!(context);
                            lines_used += 1;
                        }

                        if is_refs_only {
                            let _ = writeln!(context, "**Dependency chain** (functions called by symbols in this file):");
                        } else {
                            let _ = writeln!(context, "**Callees** (functions called by this file's symbols):");
                        }
                        lines_used += 1;

                        for (sym_name, callee_info) in &symbol_callees {
                            if lines_used >= budget {
                                break;
                            }
                            let callees_str = callee_info
                                .iter()
                                .map(|(name, file)| format!("`{}` ({})", name, file))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let _ = writeln!(context, "- `{}` calls → {}", sym_name, callees_str);
                            lines_used += 1;
                        }
                    }
                }
            }
            } // end if let Ok(refs_vs)
        }
    }

    // 10. Evaluate reaction rules
    let mut reactions_fired = 0usize;
    let mut rules_fired: Vec<String> = Vec::new();
    let mut rules_deduped = 0usize;
    if has_reactions {
        // Open MetadataStore for coupling reactions (may already be open above, but
        // the store is cheap to reopen and this path also serves ReactionsOnly mode)
        let reaction_metadata = MetadataStore::open(&db_path).ok();
        let reaction_budget = budget.saturating_sub(lines_used);

        let eval_result = reactions::evaluate_reactions(
            &tool_event,
            &compiled_rules,
            &mut dedup,
            reaction_metadata.as_ref(),
            reaction_budget,
            &role,
        );

        if !eval_result.output.is_empty() {
            if !context.is_empty() {
                context.push('\n');
                lines_used += 1;
            }
            context.push_str(&eval_result.output);
            lines_used += eval_result.output.lines().count();
        }

        reactions_fired = eval_result.reactions_fired;
        rules_fired = eval_result.rules_fired.clone();
        rules_deduped = eval_result.rules_deduped;

        // Emit per-rule metrics with injection_ids
        for (rule_name, inj_id) in eval_result.rules_fired.iter().zip(&eval_result.injection_ids) {
            crate::metrics::emit(
                &repo_root,
                &crate::metrics::event(
                    &metrics_source,
                    "reaction_fired",
                    rule_name,
                    0,
                    serde_json::json!({
                        "tool_name": input.tool_name,
                        "rule": rule_name,
                        "injection_id": inj_id,
                    }),
                ),
            );
        }
    }

    // Skip if nothing useful to report across all sections
    if context.is_empty() {
        let dispatch_label = match &mode {
            DispatchMode::EditRelated { file_path } => file_path.clone(),
            DispatchMode::SearchQuery { original_cmd, .. } => original_cmd.clone(),
            DispatchMode::RefsOnly { file_path } => file_path.clone(),
            DispatchMode::ReactionsOnly => input.tool_name.clone(),
        };
        crate::metrics::emit(
            &repo_root,
            &crate::metrics::event(
                &metrics_source,
                "hook_post_tool_use",
                "hook post-tool-use",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({
                    "tool_name": input.tool_name,
                    "dispatch": dispatch_label,
                    "coupled_count": 0,
                    "search_files": 0,
                    "refs_count": 0,
                    "callees_count": 0,
                    "reactions_fired": 0,
                    "skipped": true,
                }),
            ),
        );
        return Ok(());
    }

    // 11. Output hook response JSON
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUse".to_string(),
            additional_context: context,
        },
    };
    println!("{}", serde_json::to_string(&response)?);

    // 12. Emit metric
    let dispatch_label = match &mode {
        DispatchMode::EditRelated { file_path } => file_path.clone(),
        DispatchMode::SearchQuery { original_cmd, .. } => original_cmd.clone(),
        DispatchMode::RefsOnly { file_path } => file_path.clone(),
        DispatchMode::ReactionsOnly => input.tool_name.clone(),
    };
    crate::metrics::emit(
        &repo_root,
        &crate::metrics::event(
            &metrics_source,
            "hook_post_tool_use",
            "hook post-tool-use",
            hook_start.elapsed().as_millis() as u64,
            serde_json::json!({
                "tool_name": input.tool_name,
                "dispatch": dispatch_label,
                "coupled_count": coupled_count,
                "search_files": search_file_count,
                "refs_count": refs_count,
                "callees_count": callees_count,
                "reactions_fired": reactions_fired,
                "reactions_rules": rules_fired,
                "reactions_deduped": rules_deduped,
            }),
        ),
    );

    Ok(())
}
