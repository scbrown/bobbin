//! `tool_response` parsing for the PostToolUse hook (GitHub issue #51, Feature 1).
//!
//! bobbin has always seen what a tool was ASKED to do (`tool_input`) and never what it
//! FOUND. These helpers turn a Claude Code `tool_response` into repo-relative paths so
//! the hook can answer "related to what you found" instead of only "related to what you
//! asked for".
//!
//! Out of `src/cli/hook.rs` for the reason `hook_text_helpers_tests.rs` states: that
//! file is frozen at its current size by `scripts/large-file-allowlist.txt`, and the
//! sanctioned way to add to it is to split a block out rather than raise the ceiling.
//!
//! The response shapes each arm parses are MEASURED, not guessed — see
//! `extract_files_from_tool_response`.

use std::path::{Path, PathBuf};

use super::{clean_regex_for_search, extract_search_query_from_bash, is_meaningful_search_query};

/// Check if a file path points to source code (where symbol refs are useful).
///
/// Moved here from `hook.rs` with the extractors that gate on it — it is the one
/// classification both the Read dispatch and every discovered path go through.
pub(crate) fn is_source_code_file(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(
        ext,
        "rs" | "go"
            | "py"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
            | "rb"
            | "swift"
            | "kt"
            | "scala"
            | "zig"
            | "lua"
            | "ex"
            | "exs"
            | "erl"
            | "hs"
            | "ml"
            | "mli"
            | "fs"
            | "fsi"
    )
}
/// How much of a Bash `stdout` is scanned for paths. Unbounded scanning of arbitrary
/// command output is the one way this feature could cost real time in the hook path.
const BASH_STDOUT_SCAN_LIMIT: usize = 64 * 1024;

/// How many discovered files survive into a dispatch. Coupling is one store lookup per
/// file, and past five the injection stops being a hint and starts being a list.
const MAX_DISCOVERED_FILES: usize = 5;

/// Parse the paths a tool actually FOUND out of its `tool_response`.
///
/// The shapes below are MEASURED, not guessed. The plan for this feature
/// (docs/plans/breadcrumb-system.md, Phase 1 step 2) said to log response shapes to
/// metrics first because the exact JSON from Claude Code was unconfirmed; that data
/// already existed, in desire-path's capture of every PostToolUse payload, so each arm
/// below is written against real captured JSON. The single unmeasured arm says so.
///
/// This is a PURE parser: it does not touch the filesystem. Deciding whether a parsed
/// path is a real file inside the repo is `resolve_discovered_files`'s job. Keeping the
/// split means the parsing is testable without a fixture repo, and the noisy Bash arm
/// is filtered by something stronger than a regex.
pub(crate) fn extract_files_from_tool_response(
    tool_name: &str,
    tool_response: &serde_json::Value,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // The explicit list form: Grep in `files_with_matches` mode, and Glob.
    if let Some(arr) = tool_response.get("filenames").and_then(|v| v.as_array()) {
        out.extend(
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| looks_like_discovered_path(s))
                .map(|s| s.to_string()),
        );
    }

    match tool_name {
        "Grep" => {
            // MEASURED: in `content` mode `filenames` is EMPTY and every path is inside
            // `content`, as `path:line:text`. An extractor that reads only `filenames`
            // therefore finds nothing in the mode Grep is most often called in — a
            // silent no-op, which is the exact failure this feature exists to end.
            if out.is_empty() {
                if let Some(content) = tool_response.get("content").and_then(|v| v.as_str()) {
                    out.extend(content.lines().filter_map(match_line_path));
                }
            }
        }
        "Glob" => {
            // UNMEASURED. No Glob call appears in the captured payloads on the host this
            // was written against, because `grep` there is a shell function and path
            // searches arrive as Bash. `filenames` above is the documented shape; these
            // fallbacks are defensive and cost nothing when it holds.
            if out.is_empty() {
                if let Some(arr) = tool_response.as_array() {
                    out.extend(
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .filter(|s| looks_like_discovered_path(s))
                            .map(|s| s.to_string()),
                    );
                }
            }
            if out.is_empty() {
                out.extend(newline_separated_paths(tool_response));
            }
        }
        "Bash" => {
            // MEASURED: {stdout, stderr, interrupted, isImage, noOutputExpected}. stdout
            // is arbitrary text, so this is by far the lowest-precision arm: it is
            // bounded here and filtered against the filesystem at the call site.
            if let Some(stdout) = tool_response.get("stdout").and_then(|v| v.as_str()) {
                for line in truncate_on_char_boundary(stdout, BASH_STDOUT_SCAN_LIMIT).lines() {
                    // `grep -n` emits `path:line:text`; `find`/`ls` emit a bare path.
                    if let Some(path) = match_line_path(line) {
                        out.push(path);
                        continue;
                    }
                    out.extend(bare_path_tokens(line));
                }
            }
        }
        _ => {}
    }

    out
}

/// Pull the path out of a `path:line:text` match line, as emitted by Grep's `content`
/// mode and by `grep -n` / `rg -n`.
///
/// The line number is REQUIRED to be numeric. Without that check any prose containing a
/// colon parses as a match line, and ordinary command output is full of those.
fn match_line_path(line: &str) -> Option<String> {
    let (candidate, rest) = line.split_once(':')?;
    let lineno = rest.split_once(':').map(|(n, _)| n).unwrap_or(rest);
    if lineno.is_empty() || !lineno.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let candidate = candidate.trim();
    looks_like_discovered_path(candidate).then(|| candidate.to_string())
}

/// Whitespace-separated path-looking tokens from one line of command output.
fn bare_path_tokens(line: &str) -> Vec<String> {
    line.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | ',' | ';' | ':'))
        })
        .filter(|t| looks_like_discovered_path(t))
        .map(|t| t.to_string())
        .collect()
}

/// Newline-separated paths from a response that is either a bare string or carries one.
fn newline_separated_paths(tool_response: &serde_json::Value) -> Vec<String> {
    let text = match tool_response {
        serde_json::Value::String(s) => s.as_str(),
        v => v.get("content").and_then(|c| c.as_str()).unwrap_or(""),
    };
    text.lines()
        .map(str::trim)
        .filter(|l| looks_like_discovered_path(l))
        .map(|l| l.to_string())
        .collect()
}

/// A cheap shape gate applied before any filesystem access.
///
/// Requiring a source extension is deliberate: a discovered path is only useful here if
/// bobbin can have indexed it, and it is also the cheapest defence against Bash stdout.
fn looks_like_discovered_path(token: &str) -> bool {
    if token.is_empty() || token.len() > 512 {
        return false;
    }
    if token.contains(['*', '?']) {
        return false;
    }
    is_source_code_file(token)
}

/// Truncate without splitting a UTF-8 code point. Slicing `stdout` by byte index would
/// panic on any multi-byte character straddling the limit.
fn truncate_on_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Turn parsed paths into repo-relative paths that actually exist inside this repo.
///
/// Existence is the precision lever for the Bash arm: a token that merely looks like a
/// path is discarded here rather than being sent to the coupling store. Returned paths
/// are repo-relative because that is what the coupling store keys on.
pub(crate) fn resolve_discovered_files(
    raw: Vec<String>,
    cwd: &Path,
    repo_root: &Path,
) -> Vec<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut resolved: Vec<String> = Vec::new();
    for candidate in raw {
        let trimmed = candidate.strip_prefix("./").unwrap_or(&candidate);
        let abs = if Path::new(trimmed).is_absolute() {
            PathBuf::from(trimmed)
        } else {
            cwd.join(trimmed)
        };
        let Ok(abs) = abs.canonicalize() else {
            continue;
        };
        if !abs.is_file() {
            continue;
        }
        let Ok(rel) = abs.strip_prefix(&root) else {
            continue;
        };
        let rel = rel.to_string_lossy().to_string();
        if rel.is_empty() || resolved.contains(&rel) {
            continue;
        }
        resolved.push(rel);
        if resolved.len() >= MAX_DISCOVERED_FILES {
            break;
        }
    }
    resolved
}

/// Coupling limit per seed, and the cap on the merged result. Past five the injection
/// stops being a hint and starts being a list.
pub(crate) const MAX_COUPLED_FILES: usize = 5;

/// Minimum coupling score worth reporting. Matches the existing Edit-mode threshold.
const MIN_COUPLING_SCORE: f32 = 0.1;

/// Merge the coupling of several seed files into one ranked list.
///
/// Coupling is a per-file lookup, so a dispatch with several seeds (the files a search
/// FOUND) needs the results folded together: a file reachable from two seeds keeps its
/// better score rather than appearing twice, and a file that is itself a seed is
/// dropped — the agent already has it, so offering it back is not news.
///
/// The lookup is passed in rather than the store, so this is testable without a repo.
pub(crate) fn merge_coupling_for_seeds<F>(seeds: &[String], mut lookup: F) -> Vec<(String, f32)>
where
    F: FnMut(&str) -> Vec<crate::types::FileCoupling>,
{
    let mut merged: Vec<(String, f32)> = Vec::new();
    for seed in seeds {
        for c in lookup(seed) {
            if c.score < MIN_COUPLING_SCORE {
                continue;
            }
            let other = if c.file_a == *seed {
                c.file_b.clone()
            } else {
                c.file_a.clone()
            };
            if seeds.contains(&other) {
                continue;
            }
            match merged.iter_mut().find(|(f, _)| *f == other) {
                Some(existing) => existing.1 = existing.1.max(c.score),
                None => merged.push((other, c.score)),
            }
        }
    }
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(MAX_COUPLED_FILES);
    merged
}

/// Render the "related to what you found" section.
///
/// Returns the text and the number of lines it used, so the caller's budget accounting
/// stays in one place. `available` is the caller's remaining budget; `separate` asks for
/// a blank line first because something was already written above.
pub(crate) fn render_discovered_coupling(
    discovered: &[String],
    coupled: &[(String, f32)],
    available: usize,
    separate: bool,
) -> (String, usize) {
    use std::fmt::Write;

    // The header costs lines too, so a budget that cannot fit the header plus one row
    // buys nothing — say nothing rather than spend the remainder on a title.
    let header_lines = if separate { 4 } else { 3 };
    if coupled.is_empty() || available < header_lines + 1 {
        return (String::new(), 0);
    }

    let mut out = String::new();
    let mut used = 0;
    if separate {
        let _ = writeln!(out);
        used += 1;
    }
    let found = discovered
        .iter()
        .map(|f| format!("`{}`", f))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "## Related to What You Found");
    let _ = writeln!(
        out,
        "Your search matched {} — these co-change with them (from git history):\n",
        found
    );
    used += 3;

    for (file, score) in coupled {
        if used >= available {
            break;
        }
        let _ = writeln!(out, "- `{}` (coupling: {:.2})", file, score);
        used += 1;
    }
    (out, used)
}

/// Which built-in dispatch the PostToolUse hook will run.
pub(crate) enum DispatchMode {
    EditRelated {
        file_path: String,
    },
    SearchQuery {
        query: String,
        original_cmd: String,
    },
    RefsOnly {
        file_path: String,
    },
    ReactionsOnly, // Unknown tool — only reactions, no built-in dispatch
    /// A search whose `tool_response` named real files in this repo: the query
    /// results are still reported, and the files the search actually FOUND get
    /// their coupling looked up alongside. Strictly a superset of `SearchQuery`
    /// — it is only ever reached when at least one discovered file resolved.
    DiscoveredFiles {
        files: Vec<String>,
        query: String,
        original_cmd: String,
    },
}

/// Choose a dispatch from the tool name and what the tool was ASKED to do.
///
/// Moved out of `run_post_tool_use_inner` intact: `src/cli/hook.rs` is frozen at its
/// recorded size by `scripts/large-file-allowlist.txt`, and splitting a block out is
/// the sanctioned way to add to it, so this feature pays for its own lines. Behaviour
/// is unchanged — the caller upgrades a `SearchQuery` to `DiscoveredFiles` afterwards,
/// once the repo root is known and a discovered path can be checked against it.
pub(crate) fn classify_dispatch(tool_name: &str, tool_input: &serde_json::Value) -> DispatchMode {
    match tool_name {
        "Edit" | "Write" => {
            let file_path = tool_input
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
            let command = tool_input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match extract_search_query_from_bash(command) {
                Some(query) if is_meaningful_search_query(&query) => DispatchMode::SearchQuery {
                    query,
                    original_cmd: command.to_string(),
                },
                _ => DispatchMode::ReactionsOnly,
            }
        }
        "Grep" => {
            // Claude Code's built-in Grep tool
            let pattern = tool_input
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
            let pattern = tool_input
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
            let file_path = tool_input
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
    }
}

#[cfg(test)]
#[path = "hook_tool_response_tests.rs"]
mod hook_tool_response_tests;
