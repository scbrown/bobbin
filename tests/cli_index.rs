mod common;

use common::TestProject;
use predicates::prelude::*;

#[test]
fn index_fails_without_init() {
    let project = TestProject::new();
    project.write_rust_fixtures();

    TestProject::bobbin_cmd()
        .arg("index")
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not initialized"));
}

#[test]
fn index_rust_files() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    if !project.bobbin_index() { return };

    // Verify via status
    let output = TestProject::bobbin_cmd()
        .args(["--json", "status"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let total_files = json["stats"]["total_files"].as_u64().unwrap();
    let total_chunks = json["stats"]["total_chunks"].as_u64().unwrap();

    assert!(total_files >= 2, "expected at least 2 indexed files, got {total_files}");
    assert!(total_chunks >= 4, "expected at least 4 chunks, got {total_chunks}");
}

#[test]
fn index_json_output() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    // Check if ONNX runtime is available via plain index first
    if !project.bobbin_index() { return };

    let output = TestProject::bobbin_cmd()
        .args(["--json", "index", "--force"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "indexed");
    assert!(json["files_indexed"].as_u64().unwrap() >= 2);
    let chunks_created = json["chunks_created"].as_u64().unwrap();
    assert!(chunks_created >= 4, "expected at least 4 chunks_created, got {chunks_created}");
    assert!(json["total_chunks"].as_u64().unwrap() >= 4, "expected at least 4 total chunks in store");
}

#[test]
fn index_incremental_skips_unchanged_files() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    if !project.bobbin_index() { return };

    // Re-index without changes — should skip everything (0 files indexed)
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "up_to_date", "unchanged files should be skipped");
    assert_eq!(json["files_indexed"], 0, "no files should be re-indexed");
}

/// bo-f61: a no-change re-index with commits enabled must still fall through and
/// index git commits, instead of taking the `total_files == 0` up-to-date fast
/// path (which previously skipped the commits + beads blocks entirely). The beads
/// block shares the exact same early-return gate, so this exercises that gate
/// without requiring Dolt infrastructure.
#[test]
fn index_zero_files_still_indexes_commits() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    // First index establishes file hashes (skip if ONNX runtime unavailable).
    if !project.bobbin_index() { return };

    // Enable commit indexing in the project config (generated config disables it).
    let config_path = project.path().join(".bobbin/config.toml");
    let cfg = std::fs::read_to_string(&config_path).unwrap();
    assert!(cfg.contains("commits_enabled = false"), "expected commits disabled in generated config");
    let cfg = cfg.replace("commits_enabled = false", "commits_enabled = true");
    std::fs::write(&config_path, cfg).unwrap();

    // Re-index with NO changed source files. Pre-fix this returned "up_to_date"
    // and skipped commits; now it must fall through to the commits block.
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        json["status"], "indexed",
        "0-file index with commits enabled must fall through, not fast-return up_to_date"
    );
    assert_eq!(json["files_indexed"], 0, "no source files changed");
    assert!(
        json["commits_indexed"].as_u64().unwrap_or(0) >= 1,
        "commits should be indexed on the 0-file pass, got {:?}",
        json["commits_indexed"]
    );
}

#[test]
fn index_incremental_reindexes_modified_file() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    if !project.bobbin_index() { return };

    // Modify one file
    project.write_file("src/lib.rs", "pub fn modified() -> bool { true }\npub fn another() -> i32 { 42 }\n");

    // Re-index — should pick up the changed file
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "indexed");
    let files_indexed = json["files_indexed"].as_u64().unwrap();
    assert_eq!(files_indexed, 1, "only the modified file should be re-indexed");
}

#[test]
fn index_incremental_flag_backwards_compat() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    if !project.bobbin_index() { return };

    // --incremental flag should still work (now a no-op since it's the default)
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index", "--incremental"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "up_to_date");
}

#[test]
fn index_force_reindexes_all() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    if !project.bobbin_index() { return };

    // Force reindex
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index", "--force"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let files_indexed = json["files_indexed"].as_u64().unwrap();
    assert!(files_indexed >= 2, "force should reindex all files, got {files_indexed}");
}

#[test]
fn index_multi_language() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.write_python_fixtures();
    project.write_markdown_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    // Check ONNX runtime available, then get JSON output via force reindex
    if !project.bobbin_index() { return };

    let output = TestProject::bobbin_cmd()
        .args(["--json", "index", "--force"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let files_indexed = json["files_indexed"].as_u64().unwrap();
    // 2 rust + 1 python + 1 markdown = 4 files minimum
    assert!(
        files_indexed >= 4,
        "expected at least 4 indexed files (rust+python+md), got {files_indexed}"
    );

    // Verify via detailed status
    let status_output = TestProject::bobbin_cmd()
        .args(["--json", "status", "--detailed"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let status: serde_json::Value = serde_json::from_slice(&status_output).unwrap();
    let languages = status["stats"]["languages"].as_array().unwrap();
    let lang_names: Vec<&str> = languages
        .iter()
        .map(|l| l["language"].as_str().unwrap())
        .collect();

    assert!(lang_names.contains(&"rust"), "expected rust in languages, got {lang_names:?}");
}
