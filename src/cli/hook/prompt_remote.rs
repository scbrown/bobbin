use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use super::OutputConfig;
use super::types::{HookInput, find_bobbin_root, generate_context_injection_id};
use super::util::{detect_repo_name, strip_system_tags, is_bead_command, is_automated_message};
use super::ledger::{SessionLedger, chunk_key};
use super::format::{format_context_response_with_bundles, format_search_fallback_header, format_search_chunk};
use super::InjectContextArgs;

/// Remote-server implementation of inject-context.
/// Uses HTTP client to search instead of opening local stores.
pub(super) async fn inject_context_remote(
    args: InjectContextArgs,
    output: &OutputConfig,
    server_url: &str,
) -> Result<()> {
    use crate::http::client::Client;

    // 1. Read stdin JSON
    let input: HookInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    // 2. Load config for hook settings (use defaults if not found)
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
    };
    let config = find_bobbin_root(&cwd)
        .and_then(|root| Config::load(&Config::config_path(&root)).ok())
        .unwrap_or_default();
    let hooks_cfg = &config.hooks;

    // Apply CLI overrides
    let min_prompt_length = args.min_prompt_length.unwrap_or(hooks_cfg.min_prompt_length);
    let budget = args.budget.unwrap_or(hooks_cfg.budget);
    let format_mode = args.format_mode.as_deref().unwrap_or(&hooks_cfg.format_mode);

    // Resolve repo root and metrics source early (needed for metrics in all paths)
    let repo_root = find_bobbin_root(&cwd).unwrap_or_else(|| cwd.clone());
    let metrics_source = crate::metrics::resolve_source(None, Some(&input.session_id));
    let hook_start = std::time::Instant::now();

    // 3. Check min prompt length
    let prompt = input.prompt.trim();
    if prompt.len() < min_prompt_length {
        return Ok(());
    }

    // 3b. Check skip prefixes (operational commands that never need context).
    // Built-in prefixes always apply; user-configured prefixes extend them.
    let prompt_lower = prompt.to_lowercase();
    const BUILTIN_SKIP_PREFIXES: &[&str] = &[
        "git ", "git push", "git pull", "git status", "git diff", "git log",
        "git commit", "git add", "git stash", "git rebase", "git merge",
        "bd ", "gt ", "cargo ", "go test", "go build", "go run",
        "npm ", "make ", "docker ", "kubectl ",
        "/", // Slash commands (Claude Code skills)
    ];
    let matches_prefix = |pl: &str| -> bool {
        if pl.len() <= 5 && !pl.ends_with(' ') {
            prompt_lower == pl
        } else {
            prompt_lower.starts_with(pl)
        }
    };
    if BUILTIN_SKIP_PREFIXES.iter().any(|p| matches_prefix(p))
        || hooks_cfg.skip_prefixes.iter().any(|p| matches_prefix(&p.to_lowercase()))
    {
        return Ok(());
    }

    // 3c. Skip injection for automated messages (patrol nudges, reactor alerts, etc.)
    if is_automated_message(prompt) {
        eprintln!("bobbin: skipped (automated message detected)");
        return Ok(());
    }

    // 3d. Clean prompt: strip <system-reminder>...</system-reminder> blocks that contain
    // system boilerplate (hook output, nudge metadata, task reminders). These pollute
    // semantic search with irrelevant terms like "patrol", "hook", "system", "reminder".
    let clean_prompt = strip_system_tags(prompt);
    let search_query = if clean_prompt.trim().is_empty() {
        // If stripping tags removed everything, the prompt was purely system content
        eprintln!("bobbin: skipped (prompt is only system tags)");
        return Ok(());
    } else {
        clean_prompt.trim()
    };

    // 3e. Short bead-command skip: "remove bo-qq5h", "show aegis-abc", etc.
    if is_bead_command(search_query) {
        eprintln!("bobbin: skipped (short bead command detected)");
        return Ok(());
    }

    // 3f. Truncate long prompts for search quality — embedding models lose focus
    // on very long inputs. Keep last 500 chars (most recent/relevant content).
    let search_query = if search_query.len() > 500 {
        // Find a word boundary near the cutpoint to avoid splitting mid-word
        let cutoff = search_query.len() - 500;
        match search_query[cutoff..].find(' ') {
            Some(pos) => &search_query[cutoff + pos + 1..],
            None => &search_query[cutoff..],
        }
    } else {
        search_query
    };

    // 3g. Query intent classification: adjust gate threshold for operational queries
    let intent = crate::search::intent::classify_intent(search_query);
    let intent_adj = crate::search::intent::intent_adjustments(intent);

    // 3h. Skip injection entirely for Operational intent. Agents running shell
    // commands (gt hook, bd ready, git push, etc.) never benefit from code context.
    // The Operational gate boost (0.15) is insufficient because semantic scores
    // for hook/mail/status keywords match gastown infrastructure code at 0.7+.
    if intent == crate::search::intent::QueryIntent::Operational {
        eprintln!("bobbin: skipped (operational intent: {:?})", intent);
        return Ok(());
    }

    // 4. Assemble context via remote server (uses full ContextAssembler on server
    //    side, including coupling expansion and provenance bridging).
    //    Falls back to /search if /context returns 404 (e.g., Traefik proxy
    //    not forwarding the endpoint).
    let client = Client::new(server_url);
    let role = crate::access::RepoFilter::resolve_role(None);

    // Keyword-triggered repo scoping: when query matches configured keywords,
    // scope search to matched repos instead of searching all repos.
    let keyword_repos = hooks_cfg.resolve_keyword_repos(search_query);
    let repo_filter = if keyword_repos.is_empty() {
        None
    } else {
        Some(keyword_repos.join(","))
    };

    // Repo affinity: boost results from the agent's current repo
    let repo_affinity = detect_repo_name(&cwd);

    // Compute per-request scoring overrides from intent classification.
    // Only send overrides when intent adjustments differ from defaults (factor != 1.0).
    let search_cfg = &config.search;
    let semantic_weight_override = if (intent_adj.semantic_weight_factor - 1.0).abs() > f32::EPSILON {
        // Direct multiplication: factor < 1.0 = more keyword, > 1.0 = more semantic
        Some((search_cfg.semantic_weight * intent_adj.semantic_weight_factor).clamp(0.0, 1.0))
    } else {
        None
    };
    let doc_demotion_override = if (intent_adj.doc_demotion_factor - 1.0).abs() > f32::EPSILON {
        // doc_demotion is a score multiplier (1.0=no demotion, 0.0=full demotion).
        // Factor modifies the demotion EFFECT: factor<1.0 = less demotion (docs more visible),
        // factor>1.0 = more demotion. Invert, scale effect, invert back.
        let effect = (1.0 - search_cfg.doc_demotion) * intent_adj.doc_demotion_factor;
        Some((1.0 - effect).clamp(0.0, 1.0))
    } else {
        None
    };
    let recency_weight_override = if (intent_adj.recency_weight_factor - 1.0).abs() > f32::EPSILON {
        // Direct multiplication: factor > 1.0 = prefer recent, < 1.0 = less recency
        Some((search_cfg.recency_weight * intent_adj.recency_weight_factor).clamp(0.0, 1.0))
    } else {
        None
    };

    // Intent-aware coupling threshold: Navigation/Operational queries need
    // tighter coupling to avoid noise; Architecture queries benefit from looser.
    let coupling_threshold = intent_adj.coupling_threshold.unwrap_or(0.15);

    let context_result = client
        .context_with_weights(
            search_query,
            Some(budget),
            Some(1),    // depth: 1 level of coupling expansion
            Some(2),    // max_coupled: 2 coupled files per seed (was 3, tightened to reduce noise)
            Some(12),   // search_limit: 12 initial results (was 15, tightened for precision)
            Some(coupling_threshold),
            repo_filter.as_deref(),
            Some(&role),
            repo_affinity.as_deref(),
            semantic_weight_override,
            doc_demotion_override,
            recency_weight_override,
        )
        .await;

    // Apply intent-based gate boost (operational queries get a higher bar)
    let base_gate = args.gate_threshold.unwrap_or(hooks_cfg.gate_threshold);
    let gate = base_gate + intent_adj.gate_boost;

    match context_result {
        Ok(resp) => {
            if resp.files.is_empty() {
                return Ok(());
            }

            // Gate check: use raw cosine score from server (not RRF-normalized chunk scores)
            let top_score = if resp.summary.top_semantic_score > 0.0 {
                resp.summary.top_semantic_score
            } else {
                // Fallback for older servers that don't return top_semantic_score
                resp.files.iter()
                    .flat_map(|f| f.chunks.iter())
                    .map(|c| c.score)
                    .fold(0.0_f32, f32::max)
            };
            if top_score < gate {
                if output.verbose {
                    eprintln!(
                        "bobbin: skipped (score={:.3} < gate={:.3}, intent={:?})",
                        top_score, gate, intent,
                    );
                }
                crate::metrics::emit(&repo_root, &crate::metrics::event(
                    &metrics_source,
                    "hook_gate_skip",
                    "hook inject-context-remote",
                    hook_start.elapsed().as_millis() as u64,
                    serde_json::json!({
                        "query": &prompt[..prompt.len().min(200)],
                        "top_score": top_score,
                        "gate_threshold": gate,
                        "intent": format!("{:?}", intent),
                        "gate_boost": intent_adj.gate_boost,
                    }),
                ));
                return Ok(());
            }

            // Session dedup: filter out chunks already injected in this session
            let repo_root = find_bobbin_root(&cwd).unwrap_or_else(|| cwd.clone());
            let mut ledger = SessionLedger::load(&repo_root, &input.session_id);
            let reducing_enabled = hooks_cfg.reducing_enabled && !input.session_id.is_empty();

            // Destructure to avoid partial-move issues
            let crate::http::client::ContextResponse { query: resp_query, budget: resp_budget, files: mut resp_files, summary: resp_summary } = resp;

            if reducing_enabled {
                // Filter chunks already seen, remove empty files
                for file in resp_files.iter_mut() {
                    let original_len = file.chunks.len();
                    file.chunks.retain(|c| {
                        let key = chunk_key(&file.path, c.start_line, c.end_line);
                        !ledger.contains(&key)
                    });
                    if file.chunks.len() < original_len {
                        eprintln!(
                            "bobbin: dedup removed {}/{} chunks from {}",
                            original_len - file.chunks.len(),
                            original_len,
                            file.path,
                        );
                    }
                }
                resp_files.retain(|f| !f.chunks.is_empty());
                if resp_files.is_empty() {
                    eprintln!("bobbin: all chunks already injected this session, skipping");
                    crate::metrics::emit(&repo_root, &crate::metrics::event(
                        &metrics_source,
                        "hook_reducing_skip",
                        "hook inject-context-remote",
                        hook_start.elapsed().as_millis() as u64,
                        serde_json::json!({
                            "query": &prompt[..prompt.len().min(200)],
                        }),
                    ));
                    return Ok(());
                }
            }

            // Cross-repo filename dedup: when the same filename appears from multiple
            // repos, keep only the one from the agent's repo (or highest scoring).
            // This prevents e.g. testing.md from 5 repos all appearing in results.
            {
                let mut seen_filenames: HashMap<String, usize> = HashMap::new();
                let mut to_remove = Vec::new();
                for (idx, file) in resp_files.iter().enumerate() {
                    let filename = file.path.rsplit('/').next().unwrap_or(&file.path).to_string();
                    if let Some(&prev_idx) = seen_filenames.get(&filename) {
                        // Duplicate filename — keep the one from agent's repo, or higher score
                        let prev = &resp_files[prev_idx];
                        let prev_is_affinity = repo_affinity.as_ref().map_or(false, |ra| {
                            prev.repo.as_deref() == Some(ra.as_str()) || prev.path.contains(ra.as_str())
                        });
                        let curr_is_affinity = repo_affinity.as_ref().map_or(false, |ra| {
                            file.repo.as_deref() == Some(ra.as_str()) || file.path.contains(ra.as_str())
                        });
                        if curr_is_affinity && !prev_is_affinity {
                            // Current is from agent's repo, remove previous
                            to_remove.push(prev_idx);
                            seen_filenames.insert(filename, idx);
                        } else if !curr_is_affinity && prev_is_affinity {
                            // Previous is from agent's repo, remove current
                            to_remove.push(idx);
                        } else if file.score > prev.score {
                            // Same affinity status, keep higher score
                            to_remove.push(prev_idx);
                            seen_filenames.insert(filename, idx);
                        } else {
                            to_remove.push(idx);
                        }
                    } else {
                        seen_filenames.insert(filename, idx);
                    }
                }
                if !to_remove.is_empty() {
                    eprintln!("bobbin: cross-repo dedup removed {} duplicate filenames", to_remove.len());
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    for idx in to_remove.into_iter().rev() {
                        resp_files.remove(idx);
                    }
                }
            }

            // Filter out files already in agent context (CLAUDE.md, AGENTS.md, etc.)
            // and static project docs that waste injection budget.
            {
                let before = resp_files.len();
                resp_files.retain(|f| {
                    let filename = f.path.rsplit('/').next().unwrap_or(&f.path);
                    !matches!(filename, "CLAUDE.md" | "AGENTS.md" | "@AGENTS.md" | "CLAUDE.local.md"
                        | "MEMORY.md" | "README.md" | "CONTRIBUTING.md" | "LICENSE.md"
                        | "QUICKSTART.md" | "FAQ.md" | "INSTALLING.md" | "UNINSTALLING.md"
                        | "TROUBLESHOOTING.md" | "RELEASING.md" | "SETUP.md")
                });
                let removed = before - resp_files.len();
                if removed > 0 {
                    eprintln!("bobbin: filtered {} already-in-context files (CLAUDE.md etc.)", removed);
                }
            }

            // Filter out design doc directories — static planning/design docs
            // produce high noise (e.g. 463 _plans/ docs overwhelming real results).
            // These are reference material, not active code context.
            {
                let before = resp_files.len();
                let design_dirs = [
                    "/_plans/", "/_design/", "/_roadmap/", "/_specs/", "/audit/",
                    "/docs/tasks/", "/docs/plans/", "/docs/design/", "/docs/designs/", "/docs/runbooks/",
                    "/crew/", "/polecats/",
                    "/memory/", "/.beads/", "/session-notes/", "/sessions/",
                ];
                let test_dirs = [
                    "/tests/", "/test/", "/__tests__/", "/spec/", "/specs/",
                    "/testdata/", "/fixtures/",
                    "/examples/", "/example/", "/samples/", "/demo/", "/demos/",
                ];
                let infra_dirs = [
                    "/.github/workflows/", "/.github/actions/",
                    "/terraform/", "/ansible/", "/helm/", "/deploy/",
                    "/.circleci/", "/.gitlab-ci",
                ];
                let design_files = ["ROADMAP.md", "DESIGN.md", "ARCHITECTURE.md", "VISION.md", "PRD.md", "CHANGELOG.md"];
                resp_files.retain(|f| {
                    let path_lower = f.path.to_lowercase();
                    // Skip if path contains a design/planning directory
                    if design_dirs.iter().any(|d| path_lower.contains(d)) {
                        return false;
                    }
                    // Skip test/example directories
                    if test_dirs.iter().any(|d| path_lower.contains(d)) {
                        return false;
                    }
                    // Skip CI/infrastructure paths
                    if infra_dirs.iter().any(|d| path_lower.contains(d)) {
                        return false;
                    }
                    // Skip known design doc filenames
                    let filename = f.path.rsplit('/').next().unwrap_or(&f.path);
                    if design_files.iter().any(|d| filename.eq_ignore_ascii_case(d)) {
                        return false;
                    }
                    // Skip test file patterns (catches test files outside /test/ dirs)
                    let fname_lower = filename.to_lowercase();
                    if fname_lower.ends_with("_test.go") || fname_lower.ends_with("_test.rs")
                        || fname_lower.ends_with(".test.ts") || fname_lower.ends_with(".test.js")
                        || fname_lower.ends_with(".spec.ts") || fname_lower.ends_with(".spec.js")
                        || fname_lower.starts_with("test_")
                        || matches!(filename, "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml"
                            | "Makefile" | "Justfile" | "Taskfile.yml")
                    {
                        return false;
                    }
                    // Skip lock files and generated output
                    if matches!(filename, "Cargo.lock" | "package-lock.json" | "yarn.lock"
                        | "pnpm-lock.yaml" | "go.sum" | "Gemfile.lock" | "poetry.lock"
                        | "composer.lock" | "Pipfile.lock")
                    {
                        return false;
                    }
                    // Skip vendored/generated directories
                    if path_lower.contains("/vendor/") || path_lower.contains("/node_modules/")
                        || path_lower.contains("/third_party/") || path_lower.contains("/dist/")
                        || path_lower.contains("/build/") || path_lower.contains("/target/")
                    {
                        return false;
                    }
                    true
                });
                let removed = before - resp_files.len();
                if removed > 0 {
                    eprintln!("bobbin: filtered {} noise path files (design/test/infra)", removed);
                }
            }

            // Cross-repo non-affinity penalty: non-affinity results need a higher
            // score to survive. This prevents leakage of unrelated code from other
            // repos (e.g. gastown Go code in bobbin context). The penalty scales by
            // intent — Architecture/Config queries get a smaller penalty (cross-repo
            // docs are sometimes relevant), while General/BugFix get a larger one.
            // Language mismatch adds an extra penalty (e.g. Go results in a Rust repo).
            {
                use crate::search::intent::QueryIntent;
                let cross_repo_penalty = match intent {
                    QueryIntent::Architecture | QueryIntent::Configuration => 0.04,
                    QueryIntent::Navigation => 0.06,
                    QueryIntent::Implementation | QueryIntent::BugFix => 0.08,
                    QueryIntent::General => 0.10,
                    QueryIntent::Operational => 0.12, // Operational rarely needs cross-repo
                };
                if let Some(ref affinity) = repo_affinity {
                    // Detect dominant language from affinity-repo results
                    let affinity_lang: Option<String> = {
                        let mut lang_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
                        for f in resp_files.iter() {
                            let is_aff = f.repo.as_deref() == Some(affinity.as_str())
                                || f.path.contains(affinity.as_str());
                            if is_aff && !f.language.is_empty() && f.language != "markdown" {
                                *lang_counts.entry(&f.language).or_insert(0) += 1;
                            }
                        }
                        lang_counts.into_iter()
                            .max_by_key(|(_, count)| *count)
                            .filter(|(_, count)| *count >= 2) // Need at least 2 files to establish dominance
                            .map(|(lang, _)| lang.to_string())
                    };

                    let before = resp_files.len();
                    let non_affinity_gate = gate + cross_repo_penalty;
                    // Language mismatch adds 0.05 extra penalty on top of cross-repo penalty
                    let lang_mismatch_penalty: f32 = 0.05;
                    resp_files.retain(|f| {
                        let is_affinity = f.repo.as_deref() == Some(affinity.as_str())
                            || f.path.contains(affinity.as_str());
                        if is_affinity {
                            true // Always keep affinity results
                        } else {
                            // Check for language mismatch
                            let effective_gate = if let Some(ref aff_lang) = affinity_lang {
                                if !f.language.is_empty()
                                    && f.language != "markdown"
                                    && f.language != *aff_lang
                                {
                                    non_affinity_gate + lang_mismatch_penalty
                                } else {
                                    non_affinity_gate
                                }
                            } else {
                                non_affinity_gate
                            };
                            // Non-affinity must have at least one chunk above the effective gate
                            f.chunks.iter().any(|c| c.score >= effective_gate)
                        }
                    });
                    let removed = before - resp_files.len();
                    if removed > 0 {
                        eprintln!(
                            "bobbin: cross-repo gate filtered {} non-affinity files (gate={:.3}, lang={:?}, intent={:?})",
                            removed, non_affinity_gate, affinity_lang, intent,
                        );
                    }
                }
            }

            // Max chunks cap: prevent context flooding when many files pass the gate.
            // Keep files in order (highest relevance first), drop trailing files once
            // total chunk count exceeds the cap.
            {
                let max_chunks: usize = 12; // Cap at 12 chunks per injection
                let mut running = 0usize;
                let mut keep = resp_files.len();
                for (i, f) in resp_files.iter().enumerate() {
                    running += f.chunks.len();
                    if running > max_chunks {
                        keep = i + 1; // Keep this file (partially over) but drop the rest
                        break;
                    }
                }
                if keep < resp_files.len() {
                    let dropped = resp_files.len() - keep;
                    eprintln!("bobbin: chunks cap dropped {} trailing files ({} chunks > {})", dropped, running, max_chunks);
                    resp_files.truncate(keep);
                }
            }

            // Rebuild response with updated counts
            let total_chunks: usize = resp_files.iter().map(|f| f.chunks.len()).sum();
            let resp = crate::http::client::ContextResponse {
                query: resp_query,
                budget: resp_budget,
                files: resp_files,
                summary: crate::http::client::ContextSummaryOutput {
                    total_files: 0, // set below
                    total_chunks,
                    ..resp_summary
                },
            };
            let resp = crate::http::client::ContextResponse {
                summary: crate::http::client::ContextSummaryOutput {
                    total_files: resp.files.len(),
                    ..resp.summary
                },
                ..resp
            };

            // Generate injection_id and format structured context output
            let injection_id = generate_context_injection_id(prompt);

            // Check for bundle keyword matches (local tags.toml, then global)
            let mut tags_config = find_bobbin_root(&cwd)
                .map(|root| crate::tags::TagsConfig::load_or_default(&crate::tags::TagsConfig::tags_path(&root)))
                .unwrap_or_default();
            if tags_config.bundles.is_empty() {
                if let Some(global_dir) = Config::global_config_dir() {
                    let global_tags = global_dir.join("tags.toml");
                    if global_tags.exists() {
                        let global = crate::tags::TagsConfig::load_or_default(&global_tags);
                        if !global.bundles.is_empty() {
                            tags_config.bundles = global.bundles;
                        }
                    }
                }
            }
            let matched_bundles: Vec<crate::tags::BundleConfig> = tags_config
                .match_bundle_keywords(search_query)
                .into_iter()
                .map(|(b, _)| b.clone())
                .collect();

            let out = format_context_response_with_bundles(&resp, budget, hooks_cfg.show_docs, &injection_id, format_mode, &matched_bundles, hooks_cfg.bundle_auto_inject, hooks_cfg.bundle_inject_lines, hooks_cfg.bundle_max_inject);
            print!("{}", out);

            // Record injected chunks in session ledger
            if reducing_enabled {
                let chunk_keys: Vec<String> = resp.files.iter()
                    .flat_map(|f| f.chunks.iter().map(|c| chunk_key(&f.path, c.start_line, c.end_line)))
                    .collect();
                ledger.record(&chunk_keys, &injection_id);
            }

            // Emit injection metric
            let files_json: Vec<String> = resp.files.iter().map(|f| f.path.clone()).collect();
            let total_chunks: usize = resp.files.iter().map(|f| f.chunks.len()).sum();
            crate::metrics::emit(&repo_root, &crate::metrics::event(
                &metrics_source,
                "hook_injection",
                "hook inject-context-remote",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({
                    "query": &prompt[..prompt.len().min(200)],
                    "top_score": top_score,
                    "gate_threshold": gate,
                    "intent": format!("{:?}", intent),
                    "gate_boost": intent_adj.gate_boost,
                    "semantic_weight_override": semantic_weight_override,
                    "doc_demotion_override": doc_demotion_override,
                    "recency_weight_override": recency_weight_override,
                    "files_returned": &files_json,
                    "chunks_returned": total_chunks,
                    "injection_id": &injection_id,
                }),
            ));

            // Store injection payload server-side (best-effort, don't block)
            let session_id = if input.session_id.is_empty() { None } else { Some(input.session_id.as_str()) };
            let _ = client.store_injection_with_output(
                &injection_id,
                session_id,
                None, // agent resolved server-side or by feedback submitter
                prompt,
                &files_json,
                total_chunks,
                budget,
                Some(&out),
            ).await;

            Ok(())
        }
        Err(_) => {
            // Fallback: /context endpoint unavailable, use /search
            let session_id = if input.session_id.is_empty() { None } else { Some(input.session_id.as_str()) };
            inject_context_remote_search_fallback(&client, search_query, budget, hooks_cfg.show_docs, gate, output, Some(&role), session_id, format_mode, repo_filter.as_deref()).await
        }
    }
}

/// Fallback: use /search when /context endpoint is unavailable (e.g., behind
/// a Traefik proxy that only forwards certain paths).
pub(super) async fn inject_context_remote_search_fallback(
    client: &crate::http::client::Client,
    prompt: &str,
    budget: usize,
    show_docs: bool,
    gate: f32,
    output: &OutputConfig,
    role: Option<&str>,
    session_id: Option<&str>,
    format_mode: &str,
    repo_filter: Option<&str>,
) -> Result<()> {
    let resp = client
        .search(prompt, "hybrid", repo_filter, 10, None, role)
        .await
        .context("Remote search failed")?;

    if resp.results.is_empty() {
        return Ok(());
    }

    // Gate check
    let top_score = resp.results.first().map(|r| r.score).unwrap_or(0.0);
    if top_score < gate {
        if output.verbose {
            eprintln!(
                "bobbin: skipped (score={:.3} < gate={:.3})",
                top_score, gate,
            );
        }
        return Ok(());
    }

    let result_count = resp.results.iter().filter(|r| r.score >= gate).count();
    if result_count == 0 {
        return Ok(());
    }

    let injection_id = generate_context_injection_id(prompt);
    let mut out = format_search_fallback_header(result_count, &injection_id, format_mode);

    let mut line_count = out.lines().count();
    for result in &resp.results {
        if result.score < gate {
            continue;
        }

        // Skip docs if show_docs is false
        if !show_docs && (result.file_path.ends_with(".md") || result.file_path.contains("/docs/")) {
            continue;
        }

        let name = result
            .name
            .as_ref()
            .map(|n| format!(" {}", n))
            .unwrap_or_default();
        let chunk_section = format_search_chunk(
            &result.file_path,
            result.start_line,
            result.end_line,
            &name,
            &result.chunk_type,
            result.score,
            &result.content_preview,
            "",
            format_mode,
        );

        let chunk_line_count = chunk_section.lines().count();
        if line_count + chunk_line_count > budget {
            break;
        }
        line_count += chunk_line_count;
        out.push_str(&chunk_section);
    }

    if format_mode == "xml" {
        out.push_str("</bobbin-context>\n");
    }

    print!("{}", out);

    // Store injection payload server-side (best-effort)
    let files_json: Vec<String> = resp.results.iter()
        .filter(|r| r.score >= 0.005)
        .map(|r| r.file_path.clone())
        .collect();
    let _ = client.store_injection_with_output(
        &injection_id,
        session_id,
        None,
        prompt,
        &files_json,
        result_count,
        budget,
        Some(&out),
    ).await;

    Ok(())
}
