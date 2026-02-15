mod common;

use assert_cmd::Command;
use common::TestProject;
use predicates::prelude::*;

#[test]
fn index_fails_without_init() {
    let project = TestProject::new();
    project.write_rust_fixtures();

    Command::new(TestProject::bobbin_bin())
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
    let output = Command::new(TestProject::bobbin_bin())
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

    let output = Command::new(TestProject::bobbin_bin())
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
    let output = Command::new(TestProject::bobbin_bin())
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
    let output = Command::new(TestProject::bobbin_bin())
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
    let output = Command::new(TestProject::bobbin_bin())
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
    let output = Command::new(TestProject::bobbin_bin())
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

    let output = Command::new(TestProject::bobbin_bin())
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
    let status_output = Command::new(TestProject::bobbin_bin())
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
