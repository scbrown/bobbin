use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};
use super::OutputConfig;
use super::types::{HookInput, find_bobbin_root, generate_context_injection_id};
use super::util::{detect_repo_name, strip_system_tags, is_bead_command, is_automated_message};
use super::state::{HookState, ChunkFrequency, load_hook_state, save_hook_state, compute_session_id};
use super::ledger::{SessionLedger, PromptHistory, chunk_key};
use super::format::format_context_for_injection;
use super::hot_topics::generate_hot_topics;
use super::InjectContextArgs;
use super::prompt_remote::inject_context_remote;

pub(super) async fn run_inject_context(args: InjectContextArgs, output: OutputConfig) -> Result<()> {
    // Route to remote handler if --server is set
    if let Some(ref server_url) = output.server {
        return match inject_context_remote(args, &output, server_url).await {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("bobbin inject-context (remote): {:#}", e);
                Ok(())
            }
        };
    }
    // Never block user prompts — any error exits silently
    match inject_context_inner(args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin inject-context: {:#}", e);
            Ok(())
        }
    }
}

/// Inner implementation that can return errors (caller swallows them).
async fn inject_context_inner(args: InjectContextArgs) -> Result<()> {
    use crate::index::Embedder;
    use crate::search::context::{BridgeMode, ContentMode, ContextAssembler, ContextConfig};
    use crate::storage::{MetadataStore, VectorStore};

    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON
    let input: HookInput = serde_json::from_reader(std::io::stdin().lock())
        .context("Failed to parse stdin JSON")?;

    // 2. Resolve effective config
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
    };

    let repo_root = find_bobbin_root(&cwd).context("Bobbin not initialized")?;
    let metrics_source = crate::metrics::resolve_source(
        None, // no CLI flag in hook context
        if input.session_id.is_empty() { None } else { Some(&input.session_id) },
    );
    let config = Config::load(&Config::config_path(&repo_root))
        .context("Failed to load bobbin config")?;
    let hooks_cfg = &config.hooks;

    // Apply CLI overrides
    let min_prompt_length = args.min_prompt_length.unwrap_or(hooks_cfg.min_prompt_length);
    let threshold = args.threshold.unwrap_or(hooks_cfg.threshold);
    let budget = args.budget.unwrap_or(hooks_cfg.budget);
    let content_mode_str = args
        .content_mode
        .as_deref()
        .unwrap_or(&hooks_cfg.content_mode);
    let content_mode = match content_mode_str {
        "full" => ContentMode::Full,
        "none" => ContentMode::None,
        _ => ContentMode::Preview,
    };
    let format_mode = args.format_mode.as_deref().unwrap_or(&hooks_cfg.format_mode);

    // 3. Check min prompt length
    let prompt = input.prompt.trim();
    if prompt.len() < min_prompt_length {
        return Ok(());
    }

    // 3b. Check skip prefixes (operational commands that never need context).
    // Built-in prefixes always apply; user-configured prefixes extend them.
    let prompt_lower = prompt.to_lowercase();
    const BUILTIN_SKIP_PREFIXES_LOCAL: &[&str] = &[
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
    if BUILTIN_SKIP_PREFIXES_LOCAL.iter().any(|p| matches_prefix(p))
        || hooks_cfg.skip_prefixes.iter().any(|p| matches_prefix(&p.to_lowercase()))
    {
        return Ok(());
    }

    // 3c. Skip injection for automated messages (patrol nudges, reactor alerts, etc.)
    // These are machine-generated and don't benefit from semantic search — they just
    // produce noise injections matching docs about "escalation", "patrol", etc.
    if is_automated_message(prompt) {
        eprintln!("bobbin: skipped (automated message detected)");
        return Ok(());
    }

    // 3d. Strip system tags and truncate prompt for search quality
    let clean_prompt = strip_system_tags(prompt);
    let search_query = if clean_prompt.trim().is_empty() {
        eprintln!("bobbin: skipped (prompt is only system tags)");
        return Ok(());
    } else {
        clean_prompt.trim()
    };
    // 3e. Short bead-command skip (local mode, mirrors remote mode)
    if is_bead_command(search_query) {
        eprintln!("bobbin: skipped (short bead command detected)");
        return Ok(());
    }

    let search_query = if search_query.len() > 500 {
        let cutoff = search_query.len() - 500;
        match search_query[cutoff..].find(' ') {
            Some(pos) => &search_query[cutoff + pos + 1..],
            None => &search_query[cutoff..],
        }
    } else {
        search_query
    };

    // 3f. Conversation-aware query: enrich with recent prompt history
    let mut prompt_history = PromptHistory::load(&repo_root, &input.session_id, 5);
    let trajectory_query = prompt_history.build_trajectory_query(search_query, 700);
    prompt_history.record(search_query);
    let search_query = trajectory_query.as_str();

    // 4. Open stores
    let lance_path = Config::lance_path(&repo_root);
    let db_path = Config::db_path(&repo_root);
    let model_dir = Config::model_cache_dir()?;

    let vector_store = VectorStore::open(&lance_path)
        .await
        .context("Failed to open vector store")?;

    if vector_store.count().await? == 0 {
        return Ok(());
    }

    let metadata_store =
        MetadataStore::open(&db_path).context("Failed to open metadata store")?;

    // 5. Check model consistency
    let current_model = config.embedding.model.as_str();
    if let Some(stored) = metadata_store.get_meta("embedding_model")? {
        if stored != current_model {
            return Ok(()); // Model mismatch — skip silently
        }
    }

    let embedder = Embedder::from_config(&config.embedding, &model_dir)
        .context("Failed to load embedding model")?;

    // 6. Assemble context (config cascade: calibration.json > config.toml > intent adjustments)
    let calibration = crate::cli::calibrate::load_calibration(&repo_root);
    let cal_sw = calibration.as_ref().map(|c| c.best_config.semantic_weight);
    let cal_dd = calibration.as_ref().map(|c| c.best_config.doc_demotion);
    let cal_rrf = calibration.as_ref().map(|c| c.best_config.rrf_k);
    let cal_hl = calibration.as_ref().and_then(|c| c.best_config.recency_half_life_days);
    let cal_rw = calibration.as_ref().and_then(|c| c.best_config.recency_weight);
    let cal_budget = calibration.as_ref().and_then(|c| c.best_config.budget_lines);
    let cal_sl = calibration.as_ref().and_then(|c| c.best_config.search_limit);
    let cal_bm = calibration.as_ref().and_then(|c| c.best_config.bridge_mode);
    let cal_bbf = calibration.as_ref().and_then(|c| c.best_config.bridge_boost_factor);

    // Query intent classification: adjust search parameters based on prompt type
    let intent = crate::search::intent::classify_intent(search_query);
    let adj = crate::search::intent::intent_adjustments(intent);

    // Skip injection entirely for Operational intent (matches remote mode).
    // Agents running shell commands never benefit from code context.
    if intent == crate::search::intent::QueryIntent::Operational {
        eprintln!("bobbin: skipped (operational intent: {:?})", intent);
        return Ok(());
    }

    // Base values from calibration or config
    let base_sw = cal_sw.unwrap_or(config.search.semantic_weight);
    let base_dd = cal_dd.unwrap_or(config.search.doc_demotion);
    let base_rw = cal_rw.unwrap_or(config.search.recency_weight);

    // Cross-agent feedback: compute file-level boost scores from prior ratings.
    // Files rated "useful" by any agent for similar queries get a score boost.
    let feedback_scores = {
        let feedback_db_path = Config::feedback_db_path(&repo_root);
        crate::storage::feedback::FeedbackStore::open(&feedback_db_path)
            .ok()
            .and_then(|fb| fb.file_feedback_scores(search_query, 0.15).ok())
            .filter(|m| !m.is_empty())
    };

    let context_config = ContextConfig {
        budget_lines: cal_budget.unwrap_or(budget),
        depth: 1,
        max_coupled: 2,    // Tightened from 3 to reduce coupled noise (matches remote mode)
        coupling_threshold: adj.coupling_threshold.unwrap_or(0.1),
        semantic_weight: (base_sw * adj.semantic_weight_factor).clamp(0.0, 1.0),
        content_mode,
        search_limit: cal_sl.unwrap_or(12), // Tightened from 20 for precision (matches remote mode)
        doc_demotion: (base_dd * adj.doc_demotion_factor).clamp(0.01, 1.0),
        recency_half_life_days: cal_hl.unwrap_or(config.search.recency_half_life_days),
        recency_weight: (base_rw * adj.recency_weight_factor).clamp(0.0, 1.0),
        rrf_k: cal_rrf.unwrap_or(config.search.rrf_k),
        bridge_mode: cal_bm.unwrap_or(BridgeMode::default()),
        bridge_boost_factor: cal_bbf.unwrap_or(0.3),
        extra_filter: None,
        tags_config: None,
        role: None,
        file_type_rules: config.file_types.clone(),
        repo_affinity: detect_repo_name(&cwd),
        repo_affinity_boost: config.hooks.repo_affinity_boost,
        max_bridged_files: 2,
        max_bridged_chunks_per_file: 1,
        repo_path_prefix: config.server.repo_path_prefix.clone(),
        feedback_scores,
        ..ContextConfig::default()
    };

    let mut assembler = ContextAssembler::new(embedder, vector_store, metadata_store, context_config);
    if let Ok(git) = crate::index::git::GitAnalyzer::new(&repo_root) {
        assembler = assembler.with_git_analyzer(git);
    }
    let bundle = assembler
        .assemble(search_query, None)
        .await
        .context("Context assembly failed")?;

    // 7. Gate check: skip entire injection if top semantic score is too low
    //    Intent-based gate boost applied (operational queries get higher bar)
    let base_gate = args.gate_threshold.unwrap_or(hooks_cfg.gate_threshold);
    let gate = base_gate + adj.gate_boost;
    if bundle.summary.top_semantic_score < gate {
        eprintln!(
            "bobbin: skipped (semantic={:.2} < gate={:.2})",
            bundle.summary.top_semantic_score, gate
        );
        crate::metrics::emit(&repo_root, &crate::metrics::event(
            &metrics_source,
            "hook_gate_skip",
            "hook inject-context",
            hook_start.elapsed().as_millis() as u64,
            serde_json::json!({
                "query": prompt,
                "top_score": bundle.summary.top_semantic_score,
                "gate_threshold": gate,
            }),
        ));
        return Ok(());
    }

    // 7b. Role-based access filtering
    let role = crate::access::RepoFilter::resolve_role(None);
    let access_filter = crate::access::RepoFilter::from_config(&config.access, &role);
    let mut bundle = bundle;
    bundle.files.retain(|f| access_filter.is_allowed(crate::access::RepoFilter::repo_from_path(&f.path)));

    // 7c. Filter out files already in agent context (CLAUDE.md, AGENTS.md, etc.)
    // and static product docs that waste injection budget.
    {
        let before = bundle.files.len();
        bundle.files.retain(|f| {
            let filename = f.path.rsplit('/').next().unwrap_or(&f.path);
            !matches!(filename, "CLAUDE.md" | "AGENTS.md" | "@AGENTS.md" | "CLAUDE.local.md"
                | "MEMORY.md" | "README.md" | "CONTRIBUTING.md" | "LICENSE.md"
                        | "QUICKSTART.md" | "FAQ.md" | "INSTALLING.md" | "UNINSTALLING.md"
                        | "TROUBLESHOOTING.md" | "RELEASING.md" | "SETUP.md")
        });
        let removed = before - bundle.files.len();
        if removed > 0 {
            eprintln!("bobbin: filtered {} already-in-context files (CLAUDE.md etc.)", removed);
        }
    }

    // 7d. Filter out design doc and audit directories — static planning/design docs
    // produce high noise. Keep in sync with remote mode and context.rs is_noise_path.
    {
        let before = bundle.files.len();
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
        bundle.files.retain(|f| {
            let path_lower = f.path.to_lowercase();
            if design_dirs.iter().any(|d| path_lower.contains(d)) {
                return false;
            }
            if test_dirs.iter().any(|d| path_lower.contains(d)) {
                return false;
            }
            if infra_dirs.iter().any(|d| path_lower.contains(d)) {
                return false;
            }
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
        let removed = before - bundle.files.len();
        if removed > 0 {
            eprintln!("bobbin: filtered {} noise path files (design/test/infra)", removed);
        }
    }

    // 7e. Cross-repo non-affinity gate: non-affinity results need a higher
    // score to survive. Mirrors the remote mode gate logic.
    // Language mismatch adds an extra penalty (e.g. Go results in a Rust repo).
    {
        use crate::search::intent::QueryIntent;
        let repo_affinity = detect_repo_name(&cwd);
        let cross_repo_penalty = match intent {
            QueryIntent::Architecture | QueryIntent::Configuration => 0.04,
            QueryIntent::Navigation => 0.06,
            QueryIntent::Implementation | QueryIntent::BugFix => 0.08,
            QueryIntent::General => 0.10,
            QueryIntent::Operational => 0.12,
        };
        if let Some(ref affinity) = repo_affinity {
            // Detect dominant language from affinity-repo results
            let affinity_lang: Option<String> = {
                let mut lang_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
                for f in bundle.files.iter() {
                    let is_aff = f.repo.as_deref() == Some(affinity.as_str())
                        || f.path.contains(affinity.as_str());
                    if is_aff && !f.language.is_empty() && f.language != "markdown" {
                        *lang_counts.entry(&f.language).or_insert(0) += 1;
                    }
                }
                lang_counts.into_iter()
                    .max_by_key(|(_, count)| *count)
                    .filter(|(_, count)| *count >= 2)
                    .map(|(lang, _)| lang.to_string())
            };

            let before = bundle.files.len();
            let non_affinity_gate = gate + cross_repo_penalty;
            let lang_mismatch_penalty: f32 = 0.05;
            bundle.files.retain(|f| {
                let is_affinity = f.repo.as_deref() == Some(affinity.as_str())
                    || f.path.contains(affinity.as_str());
                if is_affinity {
                    true
                } else {
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
                    f.chunks.iter().any(|c| c.score >= effective_gate)
                }
            });
            let removed = before - bundle.files.len();
            if removed > 0 {
                eprintln!(
                    "bobbin: cross-repo gate filtered {} non-affinity files (gate={:.3}, lang={:?}, intent={:?})",
                    removed, non_affinity_gate, affinity_lang, intent,
                );
            }
        }
    }

    // 8. Session reducing: filter out chunks already injected in this session
    let reducing_enabled = hooks_cfg.reducing_enabled && !input.session_id.is_empty();
    let dedup_enabled = !args.no_dedup && hooks_cfg.dedup_enabled;
    let dedup_session_id = compute_session_id(&bundle, threshold);

    let mut ledger = if reducing_enabled {
        SessionLedger::load(&repo_root, &input.session_id)
    } else {
        SessionLedger { entries: HashSet::new(), turn: 0, path: None }
    };

    // Count total chunks before reducing (for metrics)
    let total_chunks_before: usize = bundle.files.iter()
        .flat_map(|f| f.chunks.iter())
        .filter(|c| c.score >= threshold)
        .count();
    let previously_injected = if reducing_enabled { ledger.len() } else { 0 };

    if reducing_enabled && ledger.len() > 0 {
        // Filter out chunks already in the ledger
        for file in &mut bundle.files {
            file.chunks.retain(|c| {
                let key = chunk_key(&file.path, c.start_line, c.end_line);
                !ledger.contains(&key)
            });
        }
        bundle.files.retain(|f| !f.chunks.is_empty());
    } else if dedup_enabled && !reducing_enabled {
        // Fallback: binary dedup when reducing is disabled
        let s = load_hook_state(&repo_root);
        if s.last_session_id == dedup_session_id && !dedup_session_id.is_empty() {
            eprintln!("bobbin: skipped (session unchanged)");
            crate::metrics::emit(&repo_root, &crate::metrics::event(
                &metrics_source,
                "hook_dedup_skip",
                "hook inject-context",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({ "query": prompt }),
            ));
            return Ok(());
        }
    }

    // Count new chunks after reducing
    let new_chunks: usize = bundle.files.iter()
        .flat_map(|f| f.chunks.iter())
        .filter(|c| c.score >= threshold)
        .count();
    let reduced_count = total_chunks_before.saturating_sub(new_chunks);

    // 9. Output context (only if we have new chunks) — or try complementary expansion
    if bundle.files.is_empty() || new_chunks == 0 {
        if reducing_enabled && reduced_count > 0 {
            // 9a. Complementary expansion: find coupled files the agent hasn't seen yet
            let previously_seen_files = ledger.injected_files();
            let seen_set: HashSet<&str> = previously_seen_files.iter().map(|s| s.as_str()).collect();

            let mut complementary_files: Vec<(String, f32)> = Vec::new();
            // Reopen metadata store (original was moved into ContextAssembler)
            let comp_metadata = MetadataStore::open(&db_path).ok();
            if let Some(ref comp_ms) = comp_metadata {
            for seen_file in &previously_seen_files {
                if let Ok(coupled) = comp_ms.get_coupling(seen_file, 5) {
                    for c in coupled {
                        let other = if c.file_a == *seen_file { &c.file_b } else { &c.file_a };
                        if !seen_set.contains(other.as_str()) && c.score >= 0.1 {
                            complementary_files.push((other.clone(), c.score));
                        }
                    }
                }
            }
            } // end if let Some(comp_ms)

            // Deduplicate and sort by coupling score
            complementary_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            complementary_files.dedup_by(|a, b| a.0 == b.0);
            complementary_files.truncate(5);

            if !complementary_files.is_empty() {
                // Format complementary suggestion as context
                use std::fmt::Write as FmtWrite;
                let mut comp_context = String::new();
                let _ = writeln!(comp_context, "## Complementary Files");
                let _ = writeln!(comp_context, "You've been working with files that are coupled to these (not yet viewed):\n");
                for (file, score) in &complementary_files {
                    let _ = writeln!(comp_context, "- `{}` (coupling: {:.2})", file, score);
                }

                let injection_id = generate_context_injection_id(prompt);
                let response = serde_json::json!({
                    "hookSpecificOutput": {
                        "hookEventName": "UserPromptSubmit",
                        "additionalContext": comp_context,
                    }
                });
                println!("{}", response);

                // Record complementary files in ledger (as pseudo-entries to avoid re-suggesting)
                let comp_keys: Vec<String> = complementary_files.iter()
                    .map(|(f, _)| chunk_key(f, 0, 0)) // marker entries
                    .collect();
                ledger.record(&comp_keys, &injection_id);

                crate::metrics::emit(&repo_root, &crate::metrics::event(
                    &metrics_source,
                    "hook_complementary_expansion",
                    "hook inject-context",
                    hook_start.elapsed().as_millis() as u64,
                    serde_json::json!({
                        "query": prompt,
                        "total_chunks": total_chunks_before,
                        "previously_injected": reduced_count,
                        "complementary_files": complementary_files.len(),
                    }),
                ));
                return Ok(());
            }

            eprintln!("bobbin: skipped (all {} chunks previously injected, no complementary files)", reduced_count);
            crate::metrics::emit(&repo_root, &crate::metrics::event(
                &metrics_source,
                "hook_reducing_skip",
                "hook inject-context",
                hook_start.elapsed().as_millis() as u64,
                serde_json::json!({
                    "query": prompt,
                    "total_chunks": total_chunks_before,
                    "previously_injected": reduced_count,
                }),
            ));
        }
        return Ok(());
    }

    let show_docs = args.show_docs.unwrap_or(hooks_cfg.show_docs);
    let injection_id = generate_context_injection_id(prompt);
    let context_text = format_context_for_injection(&bundle, threshold, show_docs, Some(&injection_id), format_mode);

    // If reducing is active and we filtered some chunks, show delta stats
    if reducing_enabled && reduced_count > 0 {
        eprintln!(
            "bobbin: injecting {} new chunks ({} previously injected, turn {})",
            new_chunks, reduced_count, ledger.turn + 1
        );
    }

    print!("{}", context_text);

    // Store injection record locally (best-effort)
    let feedback_db_path = Config::feedback_db_path(&repo_root);
    if let Ok(fb_store) = crate::storage::feedback::FeedbackStore::open(&feedback_db_path) {
        let files_json: Vec<String> = bundle.files.iter().map(|f| f.path.clone()).collect();
        let session_id = if input.session_id.is_empty() { None } else { Some(input.session_id.as_str()) };
        let _ = fb_store.store_injection_with_output(
            &injection_id,
            session_id,
            None,
            prompt,
            &files_json,
            new_chunks,
            bundle.budget.max_lines,
            Some(&context_text),
        );

    }

    // 10. Update hook state + session ledger
    let mut state = load_hook_state(&repo_root);
    let all_chunk_keys: Vec<String> = bundle
        .files
        .iter()
        .flat_map(|f| {
            f.chunks
                .iter()
                .filter(|c| c.score >= threshold)
                .map(move |c| (f.path.clone(), c))
        })
        .map(|(path, c)| {
            let key = chunk_key(&path, c.start_line, c.end_line);
            let freq = state.chunk_frequencies.entry(key.clone()).or_insert(ChunkFrequency {
                count: 0,
                file: path.clone(),
                name: c.name.clone(),
            });
            freq.count += 1;
            *state.file_frequencies.entry(path).or_insert(0) += 1;
            key
        })
        .collect();

    // Record in session ledger for progressive reducing
    if reducing_enabled {
        ledger.record(&all_chunk_keys, &injection_id);
    }

    state.last_session_id = dedup_session_id;
    state.last_injected_chunks = all_chunk_keys;
    state.last_injection_time = chrono::Utc::now().to_rfc3339();
    state.injection_count += 1;
    save_hook_state(&repo_root, &state);

    // 10b. Feedback prompt: periodically remind about unrated injections
    let prompt_interval = hooks_cfg.feedback_prompt_interval;
    if prompt_interval > 0 && state.injection_count % prompt_interval == 0 && !input.session_id.is_empty() {
        if let Ok(fb_store) = crate::storage::feedback::FeedbackStore::open(&feedback_db_path) {
            if let Ok(unrated) = fb_store.unrated_injections_for_session(&input.session_id) {
                if !unrated.is_empty() {
                    let sample: Vec<&str> = unrated.iter().take(3).map(|s| s.as_str()).collect();
                    eprintln!(
                        "bobbin: {} unrated injections this session. Rate with: bobbin feedback submit --injection {} --rating <useful|noise|harmful>",
                        unrated.len(),
                        sample.join(" or ")
                    );
                }
            }
        }
    }

    // 10c. Emit hook_injection metric (with reducing stats)
    let injected_files: Vec<&str> = bundle.files.iter().map(|f| f.path.as_str()).collect();
    crate::metrics::emit(&repo_root, &crate::metrics::event(
        &metrics_source,
        "hook_injection",
        "hook inject-context",
        hook_start.elapsed().as_millis() as u64,
        serde_json::json!({
            "query": prompt,
            "files_returned": injected_files,
            "chunks_returned": new_chunks,
            "top_score": bundle.summary.top_semantic_score,
            "budget_lines_used": bundle.budget.used_lines,
            "source_files": bundle.summary.source_files,
            "doc_files": bundle.summary.doc_files,
            "bridged_additions": bundle.summary.bridged_additions,
            "reducing": {
                "enabled": reducing_enabled,
                "total_before": total_chunks_before,
                "new_chunks": new_chunks,
                "previously_injected": reduced_count,
                "ledger_size": ledger.len(),
                "turn": ledger.turn,
            },
        }),
    ));

    // 11. Auto-generate hot topics every 10 injections
    if state.injection_count % 10 == 0
        && state.injection_count > state.hot_topics_generated_at
    {
        let hot_topics_path = repo_root.join(".bobbin").join("hot-topics.md");
        if generate_hot_topics(&state, &hot_topics_path).is_ok() {
            // Update the generation marker (re-load to avoid stale writes)
            let mut updated = load_hook_state(&repo_root);
            updated.hot_topics_generated_at = state.injection_count;
            save_hook_state(&repo_root, &updated);
            eprintln!("bobbin: regenerated hot-topics.md ({} injections)", state.injection_count);
        }
    }

    Ok(())
}
