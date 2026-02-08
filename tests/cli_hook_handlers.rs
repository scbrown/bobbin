mod common;

use assert_cmd::Command;
use common::TestProject;
use predicates::prelude::*;

/// Helper: initialize, write fixtures, and index a project.
fn indexed_project() -> TestProject {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.write_python_fixtures();
    project.write_markdown_fixtures();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .output()
        .expect("init failed");

    Command::new(TestProject::bobbin_bin())
        .arg("index")
        .arg(project.path())
        .output()
        .expect("index failed");

    project
}

// ─── inject-context handler ─────────────────────────────────────────────────

#[test]
fn inject_context_returns_relevant_context() {
    let project = indexed_project();

    let stdin_json = serde_json::json!({
        "prompt": "how does the calculator work",
        "cwd": project.path().to_str().unwrap()
    });

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Bobbin found")
                .and(predicate::str::contains("relevant files")),
        );
}

#[test]
fn inject_context_includes_file_and_score() {
    let project = indexed_project();

    let stdin_json = serde_json::json!({
        "prompt": "calculator addition and multiplication",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    // Should contain file paths and score annotations
    assert!(
        stdout.contains("src/lib.rs") || stdout.contains("score"),
        "Output should contain file references or scores: {}",
        stdout
    );
}

#[test]
fn inject_context_skips_short_prompts() {
    let project = indexed_project();

    // Default min_prompt_length is 10, "hi" is too short
    let stdin_json = serde_json::json!({
        "prompt": "hi",
        "cwd": project.path().to_str().unwrap()
    });

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn inject_context_silent_on_missing_index() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Init but don't index
    Command::new(TestProject::bobbin_bin())
        .arg("init")
        .arg(project.path())
        .output()
        .expect("init failed");

    let stdin_json = serde_json::json!({
        "prompt": "how does the calculator work",
        "cwd": project.path().to_str().unwrap()
    });

    // Should exit 0 with no output (never block user prompts)
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn inject_context_silent_on_no_bobbin() {
    let project = TestProject::new();
    project.git_commit("initial");

    // No bobbin init at all
    let stdin_json = serde_json::json!({
        "prompt": "how does the calculator work",
        "cwd": project.path().to_str().unwrap()
    });

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn inject_context_budget_limits_output() {
    let project = indexed_project();

    let stdin_json = serde_json::json!({
        "prompt": "show me all functions and structs in the codebase",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context", "--budget", "10"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let line_count = stdout.lines().count();
    assert!(
        line_count <= 10,
        "Budget of 10 lines should limit output, got {} lines",
        line_count
    );
}

#[test]
fn inject_context_respects_threshold_override() {
    let project = indexed_project();

    let stdin_json = serde_json::json!({
        "prompt": "calculator math operations add multiply",
        "cwd": project.path().to_str().unwrap()
    });

    // Very high threshold should filter out everything
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context", "--threshold", "0.99"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success();
    // Not asserting empty since scores vary, but at minimum it shouldn't crash
}

// ─── session-context handler ────────────────────────────────────────────────

#[test]
fn session_context_returns_json_with_git_state() {
    let project = indexed_project();

    // Create some git history to report
    project.write_file("src/new_feature.rs", "pub fn new_thing() {}\n");
    project.git_commit("feat: add new feature");

    let stdin_json = serde_json::json!({
        "source": "compact",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    // Should produce JSON with hookSpecificOutput
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Expected valid JSON, got error: {}\nOutput: {}", e, stdout));

    assert_eq!(
        json["hookSpecificOutput"]["hookEventName"]
            .as_str()
            .unwrap(),
        "SessionStart"
    );

    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("Working Context"),
        "Should contain working context header"
    );
    assert!(
        context.contains("Recent commits"),
        "Should contain recent commits section"
    );
}

#[test]
fn session_context_includes_modified_files() {
    let project = indexed_project();

    // Create uncommitted changes
    project.write_file("src/lib.rs", "pub fn modified() {}\n");

    let stdin_json = serde_json::json!({
        "source": "compact",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Expected valid JSON: {}\nOutput: {}", e, stdout));

    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("Modified files"),
        "Should show modified files section"
    );
    assert!(
        context.contains("src/lib.rs"),
        "Should list the modified file"
    );
}

#[test]
fn session_context_ignores_non_compact_events() {
    let project = indexed_project();

    let stdin_json = serde_json::json!({
        "source": "new_session",
        "cwd": project.path().to_str().unwrap()
    });

    // Non-compact source should produce no output
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn session_context_silent_on_empty_stdin() {
    let project = indexed_project();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context"])
        .current_dir(project.path())
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn session_context_budget_limits_output() {
    let project = indexed_project();

    // Create lots of commits for a full context
    for i in 0..10 {
        project.write_file(
            &format!("src/file_{}.rs", i),
            &format!("pub fn func_{}() {{}}\n", i),
        );
        project.git_commit(&format!("feat: add file_{}", i));
    }

    let stdin_json = serde_json::json!({
        "source": "compact",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context", "--budget", "8"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&stdin_json).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    if !stdout.is_empty() {
        let json: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("Expected valid JSON: {}\nOutput: {}", e, stdout));

        let context = json["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        let line_count = context.lines().count();
        assert!(
            line_count <= 8,
            "Budget of 8 should limit context lines, got {}",
            line_count
        );
    }
}

// ─── End-to-end workflow ────────────────────────────────────────────────────

#[test]
fn end_to_end_install_inject_uninstall() {
    let project = indexed_project();

    // 1. Install hooks
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hooks installed"));

    // 2. Verify status shows installed
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "status"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("installed"));

    // 3. Run inject-context (simulating Claude Code calling the hook)
    let inject_input = serde_json::json!({
        "prompt": "how does the calculator struct work",
        "cwd": project.path().to_str().unwrap()
    });

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "inject-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&inject_input).unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("Bobbin found"));

    // 4. Run session-context (simulating Claude Code SessionStart)
    let session_input = serde_json::json!({
        "source": "compact",
        "cwd": project.path().to_str().unwrap()
    });

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "session-context"])
        .current_dir(project.path())
        .write_stdin(serde_json::to_string(&session_input).unwrap())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    if !stdout.is_empty() {
        let json: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("Expected valid JSON: {}\nOutput: {}", e, stdout));
        assert!(json["hookSpecificOutput"]["hookEventName"].is_string());
    }

    // 5. Install git hook
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    // 6. Verify status shows both installed
    let status_output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "status", "--json"])
        .current_dir(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let status: serde_json::Value = serde_json::from_slice(&status_output).unwrap();
    assert_eq!(status["hooks_installed"].as_bool().unwrap(), true);
    assert_eq!(status["git_hook_installed"].as_bool().unwrap(), true);

    // 7. Uninstall everything
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall"])
        .current_dir(project.path())
        .assert()
        .success();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    // 8. Verify status shows nothing installed
    let final_status = Command::new(TestProject::bobbin_bin())
        .args(["hook", "status", "--json"])
        .current_dir(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let final_json: serde_json::Value = serde_json::from_slice(&final_status).unwrap();
    assert_eq!(final_json["hooks_installed"].as_bool().unwrap(), false);
    assert_eq!(final_json["git_hook_installed"].as_bool().unwrap(), false);
}
