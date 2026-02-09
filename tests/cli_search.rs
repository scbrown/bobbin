mod common;

use assert_cmd::Command;
use common::{try_indexed_project, TestProject};
use predicates::prelude::*;

// ─── Semantic Search ────────────────────────────────────────────────────────

#[test]
fn search_semantic_finds_relevant_results() {
    let Some(project) = try_indexed_project() else { return };

    Command::new(TestProject::bobbin_bin())
        .args(["search", "calculator", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Calculator").or(predicate::str::contains("calculator")));
}

#[test]
fn search_semantic_json_output_structure() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "add numbers", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["query"], "add numbers");
    assert_eq!(json["mode"], "semantic");
    assert!(json["results"].is_array());
    assert!(json["count"].as_u64().unwrap() > 0);

    // Verify result structure
    let first = &json["results"][0];
    assert!(first["file_path"].is_string());
    assert!(first["chunk_type"].is_string());
    assert!(first["score"].is_f64());
    assert!(first["start_line"].is_number());
    assert!(first["end_line"].is_number());
    assert!(first["language"].is_string());
    assert_eq!(first["match_type"].as_str().unwrap(), "semantic");
}

#[test]
fn search_limit_respected() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "function", "--limit", "2", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let count = json["results"].as_array().unwrap().len();
    assert!(count <= 2, "expected at most 2 results, got {count}");
}

#[test]
fn search_type_filter() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "calculator", "--type", "struct", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    for result in json["results"].as_array().unwrap() {
        assert_eq!(
            result["chunk_type"].as_str().unwrap(),
            "struct",
            "type filter should only return structs"
        );
    }
}

#[test]
fn search_finds_python_code() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "user management", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let results = json["results"].as_array().unwrap();
    let has_python = results.iter().any(|r| r["language"].as_str().unwrap() == "python");
    assert!(has_python, "semantic search for 'user management' should find python UserService");
}

#[test]
fn search_returns_content_preview() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "clamp value", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let results = json["results"].as_array().unwrap();
    // At least one result should have a content_preview
    let has_preview = results.iter().any(|r| r["content_preview"].is_string());
    assert!(has_preview, "results should include content_preview");
}

// ─── Grep (FTS) ─────────────────────────────────────────────────────────────
// FTS now uses single-column index on `content` (LanceDB 0.17 compatible).

#[test]
fn grep_finds_results() {
    let Some(project) = try_indexed_project() else { return };

    Command::new(TestProject::bobbin_bin())
        .args(["grep", "Calculator"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Calculator"));
}

// ─── Error cases ────────────────────────────────────────────────────────────

#[test]
fn search_fails_without_init() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .args(["search", "anything"])
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not initialized"));
}

#[test]
fn search_empty_index_returns_empty_index_message() {
    let project = TestProject::new();

    // Init but don't index
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .output()
        .expect("init failed");

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "search", "anything", "--mode", "semantic"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["error"], "empty_index");
}
