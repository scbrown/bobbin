use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use crate::config::Config;
use crate::storage::{MetadataStore, VectorStore};
use super::types::{SessionStartInput, HookResponse, HookSpecificOutput, FileSymbolInfo, SymbolInfo, find_bobbin_root};
use super::util::extract_brief;
use super::ledger::SessionLedger;
use super::format::format_session_context;
use super::{SessionContextArgs, PrimeContextArgs, OutputConfig};

pub(super) async fn run_session_context(args: SessionContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block session start — wrap everything in a catch-all
    match run_session_context_inner(args).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin session-context: {}", e);
            Ok(())
        }
    }
}

async fn run_session_context_inner(args: SessionContextArgs) -> Result<()> {
    // 1. Read stdin JSON
    let input_str = std::io::read_to_string(std::io::stdin())
        .context("Failed to read stdin")?;

    // If stdin is empty, nothing to do
    if input_str.trim().is_empty() {
        return Ok(());
    }

    let input: SessionStartInput =
        serde_json::from_str(&input_str).context("Failed to parse stdin JSON")?;

    // 2. Only handle compact events
    if input.source != "compact" {
        return Ok(());
    }

    // 3. Determine repo root
    let cwd = if input.cwd.is_empty() {
        std::env::current_dir().context("Failed to get cwd")?
    } else {
        PathBuf::from(&input.cwd)
            .canonicalize()
            .context("Invalid cwd path")?
    };

    // 3b. Reset session reducing ledger on compaction — agent lost prior context
    if !input.session_id.is_empty() {
        SessionLedger::clear(&cwd, &input.session_id);
        eprintln!("bobbin: reset reducing ledger (compaction)");
    }

    // Load config (use defaults if not initialized)
    let config = Config::load(&Config::config_path(&cwd)).unwrap_or_default();
    let budget = args.budget.unwrap_or(config.hooks.budget);

    // 4. Gather git signals
    let modified_files = git_status_files(&cwd)?;
    let recent_commits = git_recent_commits(&cwd, 5)?;
    let recently_changed_files = git_recently_changed_files(&cwd, 3)?;

    // If there's nothing to report, exit silently
    if modified_files.is_empty() && recent_commits.is_empty() && recently_changed_files.is_empty() {
        return Ok(());
    }

    // 5. Collect all interesting file paths (deduped)
    let mut all_files: HashSet<String> = HashSet::new();
    for f in &modified_files {
        all_files.insert(f.clone());
    }
    for f in &recently_changed_files {
        all_files.insert(f.clone());
    }

    // 6. Query bobbin for symbols and coupling (best-effort)
    let mut file_symbols: Vec<FileSymbolInfo> = Vec::new();
    let mut coupled_files: Vec<(String, String, f32)> = Vec::new(); // (path, coupled_to, score)

    let lance_path = Config::lance_path(&cwd);
    let db_path = Config::db_path(&cwd);

    if lance_path.exists() && db_path.exists() {
        let vector_store = match VectorStore::open(&lance_path).await {
            Ok(vs) => Some(vs),
            Err(e) => {
                eprintln!("bobbin: vector store unavailable: {}", e);
                None
            }
        };
        let metadata_store = match MetadataStore::open(&db_path) {
            Ok(ms) => Some(ms),
            Err(e) => {
                eprintln!("bobbin: metadata store unavailable: {}", e);
                None
            }
        };

        // Get symbols for each file
        if let Some(ref vs) = vector_store {
            for file_path in &all_files {
                if let Ok(chunks) = vs.get_chunks_for_file(file_path, None).await {
                    let symbols: Vec<SymbolInfo> = chunks
                        .iter()
                        .filter(|c| c.name.is_some())
                        .map(|c| SymbolInfo {
                            name: c.name.clone().unwrap_or_default(),
                        })
                        .collect();
                    if !symbols.is_empty() {
                        file_symbols.push(FileSymbolInfo {
                            path: file_path.clone(),
                            symbols,
                        });
                    }
                }
            }
        }

        // Get coupled files
        if let Some(ref ms) = metadata_store {
            let mut seen_coupled: HashSet<String> = HashSet::new();
            for file_path in &all_files {
                if let Ok(couplings) = ms.get_coupling(file_path, 3) {
                    for c in couplings {
                        let other = if c.file_a == *file_path {
                            &c.file_b
                        } else {
                            &c.file_a
                        };
                        if !all_files.contains(other) && !seen_coupled.contains(other) {
                            seen_coupled.insert(other.clone());
                            coupled_files.push((
                                other.clone(),
                                file_path.clone(),
                                c.score,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Sort file symbols by path for stable output
    file_symbols.sort_by(|a, b| a.path.cmp(&b.path));
    coupled_files.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // 7. Format compact markdown within budget
    let context_md = format_session_context(
        &modified_files,
        &recent_commits,
        &file_symbols,
        &coupled_files,
        budget,
    );

    if context_md.is_empty() {
        return Ok(());
    }

    // 8. Output hook response JSON
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "SessionStart".to_string(),
            additional_context: context_md,
        },
    };

    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// Get modified/staged/untracked files from git status
fn git_status_files(cwd: &std::path::Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .context("Failed to run git status")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            // git status --porcelain format: "XY path" where XY is a 2-char
            // status code at fixed positions 0-1, followed by a space at
            // position 2, then the path.  Do NOT trim the line first — the
            // leading space in " M" is part of the status code.
            if line.len() > 3 {
                Some(line[3..].to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(files)
}

/// Get recent commit summaries
fn git_recent_commits(cwd: &std::path::Path, count: usize) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("-{}", count)])
        .current_dir(cwd)
        .output()
        .context("Failed to run git log")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(|l| l.to_string()).collect())
}

/// Get files changed in recent commits
fn git_recently_changed_files(cwd: &std::path::Path, depth: usize) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("HEAD~{}..HEAD", depth),
        ])
        .current_dir(cwd)
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        // May fail if repo has fewer commits than depth
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

pub(super) async fn run_prime_context(_args: PrimeContextArgs, _output: OutputConfig) -> Result<()> {
    // Never block session start — swallow all errors
    match run_prime_context_inner().await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("bobbin prime-context: {}", e);
            Ok(())
        }
    }
}

async fn run_prime_context_inner() -> Result<()> {
    let hook_start = std::time::Instant::now();

    // 1. Read stdin JSON (may be empty for some Claude Code versions)
    let input_str = std::io::read_to_string(std::io::stdin())
        .context("Failed to read stdin")?;

    let session_id = if input_str.trim().is_empty() {
        String::new()
    } else {
        let input: SessionStartInput =
            serde_json::from_str(&input_str).unwrap_or(SessionStartInput {
                source: String::new(),
                cwd: String::new(),
                session_id: String::new(),
            });
        input.session_id
    };

    // 2. Find repo root
    let cwd = std::env::current_dir().context("Failed to get cwd")?;
    let repo_root = find_bobbin_root(&cwd).context("Bobbin not initialized")?;

    let metrics_source = crate::metrics::resolve_source(
        None,
        if session_id.is_empty() { None } else { Some(&session_id) },
    );

    // 3. Build primer text (brief version + live stats)
    let primer = include_str!("../../../docs/primer.md");
    let brief = extract_brief(primer);

    // 4. Gather live stats
    let lance_path = Config::lance_path(&repo_root);
    let stats_text = if let Ok(store) = VectorStore::open(&lance_path).await {
        if let Ok(stats) = store.get_stats(None).await {
            let mut lines = vec![
                format!("- {} files, {} chunks indexed", stats.total_files, stats.total_chunks),
            ];
            if !stats.languages.is_empty() {
                let langs: Vec<String> = stats.languages.iter()
                    .map(|l| format!("{} ({} files)", l.language, l.file_count))
                    .collect();
                lines.push(format!("- Languages: {}", langs.join(", ")));
            }
            lines.join("\n")
        } else {
            "- Index stats unavailable".to_string()
        }
    } else {
        "- Vector store not accessible".to_string()
    };

    // 5. Compose output
    let context = format!(
        "{}\n\n## Index Status\n{}\n\n## Available Commands\n\
        - `bobbin search <query>` — semantic + keyword hybrid search\n\
        - `bobbin context <query>` — task-aware context assembly with budget control\n\
        - `bobbin grep <pattern>` — keyword/regex search\n\
        - `bobbin related <file>` — find co-changing files via git coupling\n\
        - `bobbin refs <symbol>` — find symbol definitions and references\n\
        - `bobbin impact <file>` — predict affected files\n\
        - `bobbin hotspots` — high-churn, high-complexity files",
        brief, stats_text,
    );

    // 6. Output hook response JSON
    let response = HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "SessionStart".to_string(),
            additional_context: context,
        },
    };

    println!("{}", serde_json::to_string(&response)?);

    // 7. Emit metric
    crate::metrics::emit(&repo_root, &crate::metrics::event(
        &metrics_source,
        "hook_prime_context",
        "hook prime-context",
        hook_start.elapsed().as_millis() as u64,
        serde_json::Value::Null,
    ));

    Ok(())
}
