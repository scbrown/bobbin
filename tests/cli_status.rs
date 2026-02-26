mod common;

use assert_cmd::Command;
use common::TestProject;
use predicates::prelude::*;

#[test]
fn status_not_initialized() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .arg("status")
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not initialized"));
}

#[test]
fn status_not_initialized_json() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .args(["--json", "status"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"not_initialized\""));
}

#[test]
fn status_after_init_shows_ready() {
    let project = TestProject::new();

    // Initialize
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    // Check status
    Command::new(TestProject::bobbin_bin())
        .arg("status")
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Bobbin status for"));
}

#[test]
fn status_json_after_init() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    let output = Command::new(TestProject::bobbin_bin())
        .args(["--json", "status"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "ready");
    assert!(json["stats"]["total_files"].is_number());
    assert!(json["stats"]["total_chunks"].is_number());
}

#[test]
fn status_shows_zero_files_before_index() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    Command::new(TestProject::bobbin_bin())
        .arg("status")
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Files:        0"));
}
