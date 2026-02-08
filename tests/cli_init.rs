mod common;

use assert_cmd::Command;
use common::TestProject;
use predicates::prelude::*;

#[test]
fn init_creates_bobbin_directory() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Bobbin initialized"));

    // Verify directory structure
    assert!(project.path().join(".bobbin").exists());
    assert!(project.path().join(".bobbin/config.toml").exists());
    assert!(project.path().join(".bobbin/index.db").exists());
    assert!(project.path().join(".bobbin/vectors").exists());
}

#[test]
fn init_json_output() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .args(["--json", "init"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"initialized\""));
}

#[test]
fn init_twice_fails_without_force() {
    let project = TestProject::new();

    // First init succeeds
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    // Second init fails
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already initialized"));
}

#[test]
fn init_force_reinitializes() {
    let project = TestProject::new();

    // First init
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    // Force reinit succeeds
    Command::new(TestProject::bobbin_bin())
        .args(["init", "--force"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Bobbin initialized"));
}

#[test]
fn init_default_config_is_valid_toml() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    let config_content =
        std::fs::read_to_string(project.path().join(".bobbin/config.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&config_content).unwrap();

    // Verify essential config sections exist
    assert!(parsed.get("index").is_some());
    assert!(parsed.get("embedding").is_some());
    assert!(parsed.get("search").is_some());
}

#[test]
fn init_updates_gitignore() {
    let project = TestProject::new();

    // Create a .gitignore without .bobbin
    project.write_file(".gitignore", "target/\n");

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .assert()
        .success();

    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".bobbin"));
}

#[test]
fn init_quiet_suppresses_output() {
    let project = TestProject::new();

    Command::new(TestProject::bobbin_bin())
        .args(["--quiet", "init"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
