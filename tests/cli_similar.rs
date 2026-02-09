mod common;

use assert_cmd::Command;
use common::{try_indexed_project, TestProject};
use predicates::prelude::*;

// ─── Similar: single-target mode ────────────────────────────────────────────

#[test]
fn similar_text_query_returns_results() {
    let Some(project) = try_indexed_project() else { return };

    // Use a low threshold since the test fixtures are small
    Command::new(TestProject::bobbin_bin())
        .args(["similar", "calculator arithmetic", "--threshold", "0.5", "--path"])
        .arg(project.path())
        .assert()
        .success();
}

#[test]
fn similar_json_output_structure() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "similar", "add numbers", "--threshold", "0.5", "--path"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["mode"], "single");
    assert_eq!(json["target"], "add numbers");
    assert!(json["threshold"].is_f64());
    assert!(json["results"].is_array());
    assert!(json["count"].as_u64().is_some());

    // If we got results, verify structure
    if json["count"].as_u64().unwrap() > 0 {
        let first = &json["results"][0];
        assert!(first["file_path"].is_string());
        assert!(first["chunk_type"].is_string());
        assert!(first["similarity"].is_f64());
        assert!(first["start_line"].is_number());
        assert!(first["end_line"].is_number());
        assert!(first["language"].is_string());
        assert!(first["explanation"].is_string());
    }
}

#[test]
fn similar_threshold_filters_results() {
    let Some(project) = try_indexed_project() else { return };

    // Very high threshold should return fewer or no results
    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "similar", "calculator", "--threshold", "0.99", "--path"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    // All returned results should have similarity >= 0.99
    if let Some(results) = json["results"].as_array() {
        for r in results {
            assert!(r["similarity"].as_f64().unwrap() >= 0.99);
        }
    }
}

#[test]
fn similar_limit_respected() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "similar", "function", "--limit", "2", "--threshold", "0.5", "--path"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(json["results"].as_array().unwrap().len() <= 2);
}

// ─── Similar: scan mode ─────────────────────────────────────────────────────

#[test]
fn similar_scan_mode_works() {
    let Some(project) = try_indexed_project() else { return };

    Command::new(TestProject::bobbin_bin())
        .args(["similar", "--scan", "--path"])
        .arg(project.path())
        .assert()
        .success();
}

#[test]
fn similar_scan_json_output_structure() {
    let Some(project) = try_indexed_project() else { return };

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "similar", "--scan", "--path"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["mode"], "scan");
    assert!(json["threshold"].is_f64());
    assert!(json["clusters"].is_array());
    assert!(json["count"].as_u64().is_some());
    assert!(json["target"].is_null());
}

// ─── Similar: error cases ───────────────────────────────────────────────────

#[test]
fn similar_fails_without_init() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .args(["similar", "test query", "--path"])
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not initialized"));
}

#[test]
fn similar_no_target_and_no_scan_fails() {
    let Some(project) = try_indexed_project() else { return };

    Command::new(TestProject::bobbin_bin())
        .args(["similar", "--path"])
        .arg(project.path())
        .assert()
        .failure();
}

#[test]
fn similar_empty_index_shows_message() {
    let project = common::init_project();

    Command::new(TestProject::bobbin_bin())
        .args(["similar", "test", "--path"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No indexed content"));
}
