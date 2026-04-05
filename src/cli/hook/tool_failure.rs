use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};
use super::types::{PostToolUseFailureInput, HookResponse, HookSpecificOutput, find_bobbin_root};
use super::util::{detect_repo_name, detect_server_repo_name};
use super::format::format_context_for_injection;
use super::{PostToolUseFailureArgs, OutputConfig};

pub(super) async fn run_post_tool_use_failure(
    _args: PostToolUseFailureArgs,
    _output: OutputConfig,
) -> Result<()> {
    // Route to remote handler if --server is set
    if let Some(ref server_url) = _output.server {
        return match run_post_tool_use_failure_remote(_args, server_url, &_output.role).await {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("bobbin post-tool-use-failure (remote): {:#}", e);
                Ok(())
            }
        };
    }
    // Never block on failure handling — any error exits silently
    match run_post_tool_use_failure_inner(_args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin post-tool-use-failure: {:#}", e);
            Ok(())
        }
    }
}

/// PostToolUseFailure handler: When a tool fails, search bobbin for context
/// related to the error to help the agent recover.
async fn run_post_tool_use_failure_inner(args: PostToolUseFailureArgs) -> Result<()> {
    use crate::index::Embedder;
    use crate::search::context::{BridgeMode, ContentMode, ContextAssembler, ContextConfig};

    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON
    let input: PostToolUseFailureInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    // Skip if no error message to search with
    if input.error.trim().is_empty() {
        return Ok(());
    }

    // Fast-path: Read tool directory navigation injection
    // When Read fails on EISDIR or file-not-found, inject tree output
    if input.tool_name == "Read" {
        if let Some(output) = try_directory_navigation(&input) {
            let response = HookResponse {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PostToolUseFailure".to_string(),
                    additional_context: output,
                },
            };
            println!("{}", serde_json::to_string(&response)?);
            return Ok(());
        }
    }

    // 2. Resolve bobbin root and config
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
    let budget = args.budget.unwrap_or(config.hooks.budget / 2); // Use half budget for failure context

    let metrics_source = crate::metrics::resolve_source(
        None,
        if input.session_id.is_empty() { None } else { Some(&input.session_id) },
    );

    // 3. Extract command hint and error excerpt
    let command = input
        .tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let file_hint = input
        .tool_input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or(command);

    // Truncate error to avoid overwhelming the search
    let error_excerpt = if input.error.len() > 500 {
        &input.error[..500]
    } else {
        &input.error
    };

    // 4. Open stores
    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = match VectorStore::open(&lance_path).await {
        Ok(vs) => vs,
        Err(_) => return Ok(()),
    };

    if vector_store.count().await? == 0 {
        return Ok(());
    }

    let metadata_store = match MetadataStore::open(&db_path) {
        Ok(ms) => ms,
        Err(_) => return Ok(()),
    };

    // 5. Check model consistency
    let current_model = config.embedding.model.as_str();
    if let Some(stored) = metadata_store.get_meta("embedding_model")? {
        if stored != current_model {
            return Ok(());
        }
    }

    // 6. Try error-context injection: parse file paths from build/test errors
    //    and inject those files directly (faster + more precise than semantic search).
    let parsed = crate::errors::parse_error_output(&input.error, command);
    let mut direct_injection_output = String::new();
    let mut direct_files_found = 0usize;
    let mut direct_chunks_found = 0usize;
    let mut lines_used = 0usize;

    if input.tool_name == "Bash" && !parsed.refs.is_empty() {
        let repo_name = detect_repo_name(&cwd);

        for error_ref in &parsed.refs {
            if lines_used >= budget {
                break;
            }

            // Skip empty paths (symbol-only refs from test output)
            if error_ref.path.is_empty() {
                continue;
            }

            // Fetch chunks for this file from the index
            let chunks = vector_store.get_chunks_for_file(&error_ref.path, repo_name.as_deref()).await;
            let chunks = match chunks {
                Ok(c) if !c.is_empty() => c,
                _ => continue,
            };

            direct_files_found += 1;

            // If we have a specific line number, find the chunk containing it
            let relevant_chunks: Vec<_> = if let Some(line) = error_ref.line {
                let mut matching: Vec<_> = chunks.iter()
                    .filter(|c| c.start_line <= line && c.end_line >= line)
                    .collect();
                // If no chunk spans the exact line, take the nearest chunk
                if matching.is_empty() {
                    matching = chunks.iter()
                        .min_by_key(|c| {
                            let mid = (c.start_line + c.end_line) / 2;
                            (mid as i64 - line as i64).unsigned_abs()
                        })
                        .into_iter()
                        .collect();
                }
                matching
            } else {
                // No line number: take the first few chunks (file header / key definitions)
                chunks.iter().take(3).collect()
            };

            for chunk in &relevant_chunks {
                let chunk_lines = chunk.content.lines().count();
                if lines_used + chunk_lines + 3 > budget {
                    break;
                }

                let line_info = if let Some(line) = error_ref.line {
                    format!(" (error at line {})", line)
                } else {
                    String::new()
                };
                let symbol_info = error_ref.symbol.as_ref()
                    .map(|s| format!(" — symbol: `{}`", s))
                    .unwrap_or_default();

                direct_injection_output.push_str(&format!(
                    "### {}:{}-{}{}{}\n```{}\n{}\n```\n\n",
                    chunk.file_path,
                    chunk.start_line,
                    chunk.end_line,
                    line_info,
                    symbol_info,
                    chunk.language,
                    chunk.content.trim_end(),
                ));
                lines_used += chunk_lines + 3;
                direct_chunks_found += 1;
            }

            // Include coupled files for this error file (co-changing files)
            if lines_used < budget {
                if let Ok(coupling) = crate::reactions::query_coupling(
                    &metadata_store,
                    &error_ref.path,
                    0.3,
                    3,
                ) {
                    for coupled in &coupling.coupled_files {
                        if lines_used >= budget {
                            break;
                        }
                        // Fetch preview of coupled file
                        let coupled_chunks = vector_store
                            .get_chunks_for_file(&coupled.path, repo_name.as_deref())
                            .await;
                        if let Ok(cc) = coupled_chunks {
                            if let Some(first) = cc.first() {
                                let chunk_lines = first.content.lines().count();
                                if lines_used + chunk_lines + 3 <= budget {
                                    direct_injection_output.push_str(&format!(
                                        "### {} (coupled: {:.0}% co-change rate)\n```{}\n{}\n```\n\n",
                                        first.file_path,
                                        coupled.score * 100.0,
                                        first.language,
                                        first.content.trim_end(),
                                    ));
                                    lines_used += chunk_lines + 3;
                                    direct_chunks_found += 1;
                                    direct_files_found += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 7. If direct injection found files, output them; otherwise fall back to semantic search
    let (output_text, method) = if !direct_injection_output.is_empty() {
        let header = format!(
            "Bobbin found {} source chunks in {} files referenced by this error:\n\n",
            direct_chunks_found, direct_files_found,
        );
        (format!("{}{}", header, direct_injection_output), "direct")
    } else {
        // Semantic search fallback (original behavior)
        let query = format!(
            "{} {} error: {}",
            input.tool_name, file_hint, error_excerpt
        );
        let embedder = Embedder::from_config(&config.embedding, &model_dir)
            .context("Failed to load embedding model")?;

        let context_config = ContextConfig {
            budget_lines: budget,
            depth: 0,
            max_coupled: 0,
            coupling_threshold: 0.1,
            semantic_weight: config.search.semantic_weight,
            content_mode: ContentMode::Preview,
            search_limit: 10,
            doc_demotion: config.search.doc_demotion,
            recency_half_life_days: config.search.recency_half_life_days,
            recency_weight: config.search.recency_weight,
            rrf_k: config.search.rrf_k,
            bridge_mode: BridgeMode::Off,
            bridge_boost_factor: 0.0,
            extra_filter: None,
            tags_config: None,
            role: None,
            file_type_rules: config.file_types.clone(),
            repo_affinity: detect_repo_name(&cwd),
            repo_affinity_boost: config.hooks.repo_affinity_boost,
            max_bridged_files: 2,
            max_bridged_chunks_per_file: 1,
            repo_path_prefix: config.server.repo_path_prefix.clone(),
            ..ContextConfig::default()
        };

        let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
        let bundle = assembler.assemble(&query, None).await?;

        if bundle.files.is_empty() || bundle.summary.top_semantic_score < 0.3 {
            return Ok(());
        }

        let context_text = format_context_for_injection(&bundle, config.hooks.threshold, false, None, &config.hooks.format_mode);
        let header = format!(
            "Bobbin found {} relevant chunks for this error (via semantic search):\n\n",
            bundle.summary.total_chunks,
        );
        (format!("{}{}", header, context_text), "semantic")
    };

    // 8. Output response
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUseFailure".to_string(),
            additional_context: output_text,
        },
    };
    println!("{}", serde_json::to_string(&response)?);

    // 9. Emit metric
    crate::metrics::emit(
        &repo_root,
        &crate::metrics::event(
            &metrics_source,
            "hook_post_tool_use_failure",
            "hook post-tool-use-failure",
            hook_start.elapsed().as_millis() as u64,
            serde_json::json!({
                "tool_name": input.tool_name,
                "error_excerpt": &error_excerpt[..error_excerpt.len().min(200)],
                "method": method,
                "parsed_refs": parsed.refs.len(),
                "is_build_error": parsed.is_build_error,
                "direct_files": direct_files_found,
                "direct_chunks": direct_chunks_found,
            }),
        ),
    );

    Ok(())
}

/// Remote-server implementation of PostToolUseFailure.
/// Uses HTTP client to fetch file chunks and context instead of local stores.
async fn run_post_tool_use_failure_remote(
    args: PostToolUseFailureArgs,
    server_url: &str,
    role: &str,
) -> Result<()> {
    use crate::http::client::Client;

    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON
    let input: PostToolUseFailureInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    if input.error.trim().is_empty() {
        return Ok(());
    }

    // Fast-path: Read tool directory navigation
    if input.tool_name == "Read" {
        if let Some(output) = try_directory_navigation(&input) {
            let response = HookResponse {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PostToolUseFailure".to_string(),
                    additional_context: output,
                },
            };
            println!("{}", serde_json::to_string(&response)?);
            return Ok(());
        }
    }

    // 2. Load config for budget
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
    };

    let repo_root = find_bobbin_root(&cwd);
    let config = repo_root
        .as_ref()
        .map(|r| Config::load(&Config::config_path(r)).unwrap_or_default())
        .unwrap_or_default();
    let budget = args.budget.unwrap_or(config.hooks.budget / 2);

    let command = input
        .tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let error_excerpt = if input.error.len() > 500 {
        &input.error[..500]
    } else {
        &input.error
    };

    let client = Client::new(server_url);
    let repo_affinity = detect_repo_name(&cwd);

    // 3. Try error-context injection: parse file paths from build/test errors
    let parsed = crate::errors::parse_error_output(&input.error, command);
    let mut direct_output = String::new();
    let mut direct_files = 0usize;
    let mut direct_chunks = 0usize;
    let mut lines_used = 0usize;

    // Resolve relative paths to server paths.
    // Server stores files as /var/lib/bobbin/repos/<repo>/<path>.
    // Use config repo_path_prefix if set, otherwise default to /var/lib/bobbin/repos/.
    let repo_prefix = config.server.repo_path_prefix.as_deref()
        .unwrap_or("/var/lib/bobbin/repos");

    // Detect the actual repo name for server path resolution.
    // In Gas Town, crew workspaces are at /<town>/<rig>/crew/<name>/ — the rig
    // name is the repo name on the server, not the git root directory.
    let server_repo = detect_server_repo_name(&cwd).or(repo_affinity.clone());

    let resolve_path = |path: &str| -> String {
        if path.starts_with('/') {
            path.to_string()
        } else if let Some(ref repo) = server_repo {
            format!("{}/{}/{}", repo_prefix, repo, path.trim_start_matches("./"))
        } else {
            path.to_string()
        }
    };

    if input.tool_name == "Bash" && !parsed.refs.is_empty() {
        for error_ref in &parsed.refs {
            if lines_used >= budget || error_ref.path.is_empty() {
                continue;
            }

            let server_path = resolve_path(&error_ref.path);

            // Use read_chunk to fetch code around the error line
            let (start, end) = if let Some(line) = error_ref.line {
                // Fetch ~40 lines around the error
                (line.saturating_sub(10), line + 30)
            } else {
                // No line number — fetch the top of the file
                (1, 50)
            };

            if let Ok(chunk) = client.read_chunk(&server_path, start, end, Some(5)).await {
                let chunk_lines = chunk.content.lines().count();
                if lines_used + chunk_lines + 3 <= budget {
                    let line_info = error_ref.line
                        .map(|l| format!(" (error at line {})", l))
                        .unwrap_or_default();
                    let symbol_info = error_ref.symbol.as_ref()
                        .map(|s| format!(" — symbol: `{}`", s))
                        .unwrap_or_default();

                    direct_output.push_str(&format!(
                        "### {}:{}-{}{}{}\n```{}\n{}\n```\n\n",
                        chunk.file,
                        chunk.actual_start_line,
                        chunk.actual_end_line,
                        line_info,
                        symbol_info,
                        chunk.language,
                        chunk.content.trim_end(),
                    ));
                    lines_used += chunk_lines + 3;
                    direct_chunks += 1;
                    direct_files += 1;
                }
            }

            // Fetch coupled/related files
            if lines_used < budget {
                if let Ok(related) = client.related(&server_path, 3, Some(0.3)).await {
                    for rel in &related.related {
                        if lines_used >= budget {
                            break;
                        }
                        if let Ok(rel_chunk) = client.read_chunk(&rel.path, 1, 30, None).await {
                            let chunk_lines = rel_chunk.content.lines().count();
                            if lines_used + chunk_lines + 3 <= budget {
                                direct_output.push_str(&format!(
                                    "### {} (coupled: {:.0}% co-change rate)\n```{}\n{}\n```\n\n",
                                    rel.path,
                                    rel.score * 100.0,
                                    rel_chunk.language,
                                    rel_chunk.content.trim_end(),
                                ));
                                lines_used += chunk_lines + 3;
                                direct_chunks += 1;
                                direct_files += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Direct injection or semantic search fallback
    let (output_text, method) = if !direct_output.is_empty() {
        let header = format!(
            "Bobbin found {} source chunks in {} files referenced by this error:\n\n",
            direct_chunks, direct_files,
        );
        (format!("{}{}", header, direct_output), "direct-remote")
    } else {
        // Semantic search fallback via /context endpoint
        let query = format!(
            "{} {} error: {}",
            input.tool_name,
            command,
            &error_excerpt[..error_excerpt.len().min(200)],
        );
        match client.context(
            &query,
            Some(budget),
            Some(0),      // depth
            Some(0),      // max_coupled
            Some(10),     // limit
            None,         // coupling_threshold
            None,         // repo
            if role.is_empty() { None } else { Some(role) },
            repo_affinity.as_deref(),
        ).await {
            Ok(ctx) if !ctx.files.is_empty() => {
                let mut text = format!(
                    "Bobbin found {} relevant files for this error (via semantic search):\n\n",
                    ctx.files.len(),
                );
                for file in &ctx.files {
                    for chunk in &file.chunks {
                        text.push_str(&format!(
                            "### {}:{}-{}\n```\n{}\n```\n\n",
                            file.path,
                            chunk.start_line,
                            chunk.end_line,
                            chunk.content.as_deref().unwrap_or("").trim_end(),
                        ));
                    }
                }
                (text, "semantic-remote")
            }
            _ => return Ok(()),
        }
    };

    // 5. Output response
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PostToolUseFailure".to_string(),
            additional_context: output_text,
        },
    };
    println!("{}", serde_json::to_string(&response)?);

    // 6. Emit metric (local if we have a repo root)
    if let Some(ref root) = repo_root {
        let metrics_source = crate::metrics::resolve_source(
            None,
            if input.session_id.is_empty() { None } else { Some(&input.session_id) },
        );
        crate::metrics::emit(
            root,
            &crate::metrics::event(
                &metrics_source,
                "hook_post_tool_use_failure",
                "hook post-tool-use-failure",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({
                    "tool_name": input.tool_name,
                    "error_excerpt": &error_excerpt[..error_excerpt.len().min(200)],
                    "method": method,
                    "parsed_refs": parsed.refs.len(),
                    "is_build_error": parsed.is_build_error,
                    "direct_files": direct_files,
                    "direct_chunks": direct_chunks,
                }),
            ),
        );
    }

    Ok(())
}

/// Fast-path directory navigation: when Read fails on a directory or missing file,
/// run `tree` and return the output. Returns None if not applicable.
fn try_directory_navigation(input: &PostToolUseFailureInput) -> Option<String> {
    let file_path = input.tool_input.get("file_path").and_then(|v| v.as_str())?;
    let error = &input.error;

    // Skip paths in /tmp or other irrelevant locations
    if file_path.starts_with("/tmp") || file_path.starts_with("/proc") || file_path.starts_with("/sys") {
        return None;
    }

    let (tree_path, header) = if error.contains("EISDIR") || error.contains("Is a directory") {
        // Read on directory: show its contents
        (file_path.to_string(), format!("{} is a directory. Contents:", file_path))
    } else if error.contains("does not exist") || error.contains("ENOENT") || error.contains("No such file") {
        // File not found: show parent directory
        let parent = std::path::Path::new(file_path).parent()?;
        if !parent.exists() {
            return None;
        }
        (parent.to_string_lossy().to_string(), format!("File not found. Nearby files in {}:", parent.display()))
    } else {
        return None;
    };

    // Run tree with depth limit
    let output = std::process::Command::new("tree")
        .args(["-L", "2", "--noreport", &tree_path])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let tree_text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = tree_text.lines().collect();

    // Cap at 20 lines
    let truncated = if lines.len() > 20 {
        let shown: Vec<&str> = lines[..20].to_vec();
        format!("{}\n... and {} more entries", shown.join("\n"), lines.len() - 20)
    } else {
        lines.join("\n")
    };

    Some(format!("{}\n```\n{}\n```", header, truncated))
}
