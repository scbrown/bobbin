//! Tests for `hook_tool_response.rs` — `tool_response` parsing, discovered-file
//! resolution, coupling merge, and the "related to what you found" section.
//!
//! The response fixtures below are REAL captured payloads, not invented ones. The
//! plan for this feature (docs/plans/breadcrumb-system.md, Phase 1 step 2) said to
//! log response shapes to metrics first because the JSON Claude Code sends was
//! unconfirmed; that data already existed in desire-path's capture of every
//! PostToolUse payload, so the shapes here are copied from it. Where a shape could
//! not be measured, the test says so rather than asserting a guess.

use super::*;
use serde_json::json;

// ---------------------------------------------------------------------------
// extract_files_from_tool_response — MEASURED shapes
// ---------------------------------------------------------------------------

#[test]
fn grep_content_mode_paths_come_from_content_not_filenames() {
    // MEASURED: Grep in `content` mode returns filenames: [] and buries every path
    // in `content` as `path:line:text`. This is the whole reason the extractor
    // cannot just read `filenames` — that arm returns nothing here, silently.
    let response = json!({
        "mode": "content",
        "numFiles": 0,
        "filenames": [],
        "content": "src/search/hybrid.rs:107:        Self::combine_with_recency(\nsrc/search/hybrid.rs:125:        Self::combine_with_recency(a, b)\ndocs/plans/ppr-ranking-signal.md:52:| RRF fusion |",
        "numLines": 3,
        "totalLines": 3
    });

    let files = extract_files_from_tool_response("Grep", &response);

    // Both hits in the source file are found; the .md line is dropped by the
    // source-extension gate, which is what keeps this list indexable.
    assert_eq!(
        files,
        vec!["src/search/hybrid.rs", "src/search/hybrid.rs"],
        "content-mode paths must be parsed out of `content`"
    );
}

#[test]
fn grep_files_with_matches_mode_uses_filenames() {
    let response = json!({
        "mode": "files_with_matches",
        "numFiles": 2,
        "filenames": ["src/cli/hook.rs", "src/search/hybrid.rs"],
        "content": "",
    });
    assert_eq!(
        extract_files_from_tool_response("Grep", &response),
        vec!["src/cli/hook.rs", "src/search/hybrid.rs"]
    );
}

#[test]
fn bash_stdout_yields_both_match_lines_and_bare_paths() {
    // MEASURED shape: {stdout, stderr, interrupted, isImage, noOutputExpected}.
    let response = json!({
        "stdout": "src/cli/hook.rs:4731:    use hook_tool_response;\nsrc/breadcrumb.rs\n./src/config.rs\n",
        "stderr": "",
        "interrupted": false,
        "isImage": false,
        "noOutputExpected": false
    });
    assert_eq!(
        extract_files_from_tool_response("Bash", &response),
        vec!["src/cli/hook.rs", "src/breadcrumb.rs", "./src/config.rs"]
    );
}

#[test]
fn bash_prose_is_not_mistaken_for_paths() {
    // The precision case. Ordinary command output is full of colons and of words
    // that are not paths; none of this should reach the coupling store.
    let response = json!({
        "stdout": "note: the function is defined here\nwarning: 3 warnings emitted\nCompiling bobbin-ai v0.14.0\nerror: could not compile\n",
        "stderr": ""
    });
    assert!(
        extract_files_from_tool_response("Bash", &response).is_empty(),
        "prose with colons must not parse as `path:line:text`"
    );
}

#[test]
fn a_colon_line_without_a_line_number_is_not_a_match_line() {
    // `note: something` splits on ':' just as `path:12:text` does. The numeric
    // check on the middle field is the only thing separating them.
    assert_eq!(
        match_line_path("src/main.rs:42:    let x = 1;"),
        Some("src/main.rs".to_string())
    );
    assert_eq!(
        match_line_path("src/main.rs:42"),
        Some("src/main.rs".to_string())
    );
    assert_eq!(match_line_path("note: src/main.rs is stale"), None);
    assert_eq!(match_line_path("src/main.rs:notanumber:x"), None);
    assert_eq!(match_line_path("no colon here"), None);
}

#[test]
fn missing_tool_response_yields_nothing() {
    // Every payload predating this feature parses to Value::Null via serde(default).
    assert!(extract_files_from_tool_response("Bash", &serde_json::Value::Null).is_empty());
    assert!(extract_files_from_tool_response("Grep", &json!({})).is_empty());
    assert!(extract_files_from_tool_response("Read", &json!({"filePath": "src/a.rs"})).is_empty());
}

#[test]
fn glob_shape_is_unmeasured_so_both_documented_forms_are_accepted() {
    // UNMEASURED: no Glob call appears in the captured payloads this was written
    // against. `filenames` is the documented shape; the array and newline forms are
    // defensive. Asserting all three is cheaper than asserting the wrong one.
    assert_eq!(
        extract_files_from_tool_response("Glob", &json!({"filenames": ["src/a.rs"]})),
        vec!["src/a.rs"]
    );
    assert_eq!(
        extract_files_from_tool_response("Glob", &json!(["src/a.rs", "README.md"])),
        vec!["src/a.rs"]
    );
    assert_eq!(
        extract_files_from_tool_response("Glob", &json!("src/a.rs\nsrc/b.rs\n")),
        vec!["src/a.rs", "src/b.rs"]
    );
}

#[test]
fn stdout_scanning_is_bounded_and_utf8_safe() {
    // A byte-index truncation would panic on a multi-byte character straddling the
    // limit, in a hook that runs after every tool call.
    let s = "aé";
    assert_eq!(truncate_on_char_boundary(s, 2), "a");
    assert_eq!(truncate_on_char_boundary(s, 3), "aé");
    assert_eq!(truncate_on_char_boundary("abc", 99), "abc");

    let huge = format!("{}\nsrc/late.rs\n", "x".repeat(BASH_STDOUT_SCAN_LIMIT));
    let files = extract_files_from_tool_response("Bash", &json!({"stdout": huge}));
    assert!(
        files.is_empty(),
        "paths past the scan limit are not scanned"
    );
}

// ---------------------------------------------------------------------------
// resolve_discovered_files — the filesystem is the precision lever
// ---------------------------------------------------------------------------

#[test]
fn only_files_that_exist_inside_the_repo_survive() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/real.rs"), "fn main() {}").unwrap();

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("elsewhere.rs"), "fn main() {}").unwrap();

    let resolved = resolve_discovered_files(
        vec![
            "src/real.rs".to_string(),      // exists, in repo
            "./src/real.rs".to_string(),    // same file again — deduped
            "src/imaginary.rs".to_string(), // parses fine, does not exist
            outside
                .path()
                .join("elsewhere.rs")
                .to_string_lossy()
                .to_string(), // outside
        ],
        root,
        root,
    );

    assert_eq!(resolved, vec!["src/real.rs".to_string()]);
}

#[test]
fn discovered_files_are_capped() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let mut raw = Vec::new();
    for i in 0..(MAX_DISCOVERED_FILES + 3) {
        let name = format!("src/f{}.rs", i);
        std::fs::write(root.join(&name), "fn main() {}").unwrap();
        raw.push(name);
    }
    assert_eq!(
        resolve_discovered_files(raw, root, root).len(),
        MAX_DISCOVERED_FILES
    );
}

// ---------------------------------------------------------------------------
// merge_coupling_for_seeds
// ---------------------------------------------------------------------------

fn coupling(a: &str, b: &str, score: f32) -> crate::types::FileCoupling {
    crate::types::FileCoupling {
        file_a: a.to_string(),
        file_b: b.to_string(),
        score,
        co_changes: 1,
        last_co_change: 0,
    }
}

#[test]
fn a_file_reached_from_two_seeds_appears_once_at_its_best_score() {
    let seeds = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
    let merged = merge_coupling_for_seeds(&seeds, |seed| match seed {
        "src/a.rs" => vec![coupling("src/a.rs", "src/shared.rs", 0.4)],
        "src/b.rs" => vec![coupling("src/b.rs", "src/shared.rs", 0.9)],
        _ => vec![],
    });
    assert_eq!(merged, vec![("src/shared.rs".to_string(), 0.9)]);
}

#[test]
fn seeds_and_weak_couplings_are_dropped() {
    let seeds = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
    let merged = merge_coupling_for_seeds(&seeds, |seed| {
        if seed == "src/a.rs" {
            vec![
                // b is already in the agent's hands — offering it back is not news
                coupling("src/a.rs", "src/b.rs", 0.95),
                coupling("src/a.rs", "src/weak.rs", 0.05),
                coupling("src/a.rs", "src/good.rs", 0.5),
            ]
        } else {
            vec![]
        }
    });
    assert_eq!(merged, vec![("src/good.rs".to_string(), 0.5)]);
}

#[test]
fn merged_coupling_is_ranked_and_capped() {
    let seeds = vec!["src/a.rs".to_string()];
    let merged = merge_coupling_for_seeds(&seeds, |_| {
        (0..MAX_COUPLED_FILES + 4)
            .map(|i| {
                coupling(
                    "src/a.rs",
                    &format!("src/c{}.rs", i),
                    0.1 + i as f32 / 100.0,
                )
            })
            .collect()
    });
    assert_eq!(merged.len(), MAX_COUPLED_FILES);
    assert!(
        merged.windows(2).all(|w| w[0].1 >= w[1].1),
        "highest coupling first"
    );
}

// ---------------------------------------------------------------------------
// render_discovered_coupling
// ---------------------------------------------------------------------------

#[test]
fn rendered_section_reports_the_lines_it_used() {
    let (text, used) = render_discovered_coupling(
        &["src/found.rs".to_string()],
        &[("src/coupled.rs".to_string(), 0.42)],
        100,
        false,
    );
    assert!(text.contains("## Related to What You Found"));
    assert!(
        text.contains("`src/found.rs`"),
        "names what the search found"
    );
    assert!(text.contains("- `src/coupled.rs` (coupling: 0.42)"));
    assert_eq!(
        used,
        text.lines().count(),
        "line accounting must match the text"
    );
}

#[test]
fn a_budget_that_fits_only_the_header_renders_nothing() {
    // Spending the last lines of the budget on a title that introduces no rows is
    // worse than staying quiet.
    let (text, used) = render_discovered_coupling(
        &["src/found.rs".to_string()],
        &[("src/coupled.rs".to_string(), 0.42)],
        3,
        false,
    );
    assert!(text.is_empty());
    assert_eq!(used, 0);
}

#[test]
fn nothing_coupled_renders_nothing() {
    let (text, used) = render_discovered_coupling(&["src/found.rs".to_string()], &[], 100, true);
    assert!(text.is_empty());
    assert_eq!(used, 0);
}

// ---------------------------------------------------------------------------
// classify_dispatch — moved here with the function; behaviour unchanged
// ---------------------------------------------------------------------------

#[test]
fn read_of_a_source_file_dispatches_refs_only() {
    let mode = classify_dispatch("Read", &json!({"file_path": "src/main.rs"}));
    assert!(matches!(mode, DispatchMode::RefsOnly { .. }));
}

#[test]
fn read_of_a_non_source_file_dispatches_reactions_only() {
    let mode = classify_dispatch("Read", &json!({"file_path": "README.md"}));
    assert!(matches!(mode, DispatchMode::ReactionsOnly));
}

#[test]
fn edit_dispatches_edit_related() {
    let mode = classify_dispatch("Edit", &json!({"file_path": "src/main.rs"}));
    assert!(matches!(mode, DispatchMode::EditRelated { .. }));
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
