mod common;

use assert_cmd::Command;
use common::TestProject;
use predicates::prelude::*;

// --- bobbin hook install ---

#[test]
fn hook_install_creates_settings_json() {
    let project = TestProject::new();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hooks installed"));

    let settings_path = project.path().join(".claude").join("settings.json");
    assert!(settings_path.exists(), "settings.json should be created");

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify hook structure
    assert!(settings["hooks"]["UserPromptSubmit"].is_array());
    assert!(settings["hooks"]["SessionStart"].is_array());

    let cmd = settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(cmd, "bobbin hook inject-context");
}

#[test]
fn hook_install_idempotent() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Install twice
    for _ in 0..2 {
        Command::new(TestProject::bobbin_bin())
            .args(["hook", "install"])
            .current_dir(project.path())
            .assert()
            .success();
    }

    let content =
        std::fs::read_to_string(project.path().join(".claude").join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Should have exactly 1 entry per event, not 2
    assert_eq!(settings["hooks"]["UserPromptSubmit"].as_array().unwrap().len(), 1);
    assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
}

#[test]
fn hook_install_preserves_existing_hooks() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Write pre-existing settings with another tool's hook
    let claude_dir = project.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "other-tool inject",
            "timeout": 5
          }
        ]
      }
    ]
  },
  "customKey": "preserved"
}"#,
    )
    .unwrap();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // customKey preserved
    assert_eq!(settings["customKey"].as_str().unwrap(), "preserved");

    // Both hooks present
    let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(ups.len(), 2);
    assert_eq!(
        ups[0]["hooks"][0]["command"].as_str().unwrap(),
        "other-tool inject"
    );
    assert_eq!(
        ups[1]["hooks"][0]["command"].as_str().unwrap(),
        "bobbin hook inject-context"
    );
}

#[test]
fn hook_install_global_preserves_existing_hooks() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Set up a fake HOME with pre-existing global settings
    let fake_home = project.path().join("fakehome");
    let claude_dir = fake_home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "gt mail check --inject" }
        ],
        "matcher": ""
      }
    ],
    "SessionStart": [
      {
        "hooks": [
          { "type": "command", "command": "gt prime --hook" }
        ],
        "matcher": ""
      }
    ],
    "PreToolUse": [
      {
        "hooks": [
          { "type": "command", "command": "gt tap guard pr-workflow" }
        ],
        "matcher": "Bash(gh pr create*)"
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "gt costs record" }
        ]
      }
    ]
  },
  "statusLine": {
    "command": "bash ~/.claude/statusline-command.sh",
    "type": "command"
  }
}"#,
    )
    .unwrap();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install", "--global"])
        .env("HOME", &fake_home)
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hooks installed"));

    let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Gas Town UserPromptSubmit hook preserved alongside bobbin
    let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(ups.len(), 2);
    assert_eq!(
        ups[0]["hooks"][0]["command"].as_str().unwrap(),
        "gt mail check --inject"
    );
    assert_eq!(
        ups[1]["hooks"][0]["command"].as_str().unwrap(),
        "bobbin hook inject-context"
    );

    // Gas Town SessionStart hook preserved alongside bobbin
    let ss = settings["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(ss.len(), 2);
    assert_eq!(
        ss[0]["hooks"][0]["command"].as_str().unwrap(),
        "gt prime --hook"
    );
    assert_eq!(
        ss[1]["hooks"][0]["command"].as_str().unwrap(),
        "bobbin hook session-context"
    );

    // Events bobbin doesn't touch are completely untouched
    let ptu = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(ptu.len(), 1);
    assert_eq!(
        ptu[0]["hooks"][0]["command"].as_str().unwrap(),
        "gt tap guard pr-workflow"
    );

    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(
        stop[0]["hooks"][0]["command"].as_str().unwrap(),
        "gt costs record"
    );

    // Non-hook top-level keys preserved
    assert_eq!(
        settings["statusLine"]["command"].as_str().unwrap(),
        "bash ~/.claude/statusline-command.sh"
    );
}

#[test]
fn hook_install_global_uninstall_preserves_other_tools() {
    let project = TestProject::new();
    project.git_commit("initial");

    let fake_home = project.path().join("fakehome");
    let claude_dir = fake_home.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    // Start with Gas Town hooks
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "gt mail check --inject" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "gt costs record" }] }
    ]
  }
}"#,
    )
    .unwrap();

    // Install bobbin hooks
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install", "--global"])
        .env("HOME", &fake_home)
        .current_dir(project.path())
        .assert()
        .success();

    // Now uninstall bobbin hooks
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall", "--global"])
        .env("HOME", &fake_home)
        .current_dir(project.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Gas Town hooks should remain
    let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(ups.len(), 1);
    assert_eq!(
        ups[0]["hooks"][0]["command"].as_str().unwrap(),
        "gt mail check --inject"
    );

    let stop = settings["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1);
}

#[test]
fn hook_install_json_output() {
    let project = TestProject::new();
    project.git_commit("initial");

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "install", "--json"])
        .current_dir(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"].as_str().unwrap(), "installed");
    assert!(json["path"].as_str().unwrap().contains(".claude/settings.json"));
}

// --- bobbin hook uninstall ---

#[test]
fn hook_uninstall_removes_bobbin_hooks() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Install first
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success();

    // Uninstall
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hooks removed"));

    let content =
        std::fs::read_to_string(project.path().join(".claude").join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // hooks key should be gone (was only bobbin hooks)
    assert!(settings.get("hooks").is_none());
}

#[test]
fn hook_uninstall_no_settings_file() {
    let project = TestProject::new();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No hooks to remove"));
}

#[test]
fn hook_uninstall_preserves_other_hooks() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Create settings with both bobbin and another tool
    let claude_dir = project.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "other-tool inject" }
        ]
      },
      {
        "hooks": [
          { "type": "command", "command": "bobbin hook inject-context" }
        ]
      }
    ]
  }
}"#,
    )
    .unwrap();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall"])
        .current_dir(project.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // other-tool should remain
    let ups = settings["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(ups.len(), 1);
    assert_eq!(
        ups[0]["hooks"][0]["command"].as_str().unwrap(),
        "other-tool inject"
    );
}

// --- bobbin hook install-git-hook ---

#[test]
fn install_git_hook_creates_post_commit() {
    let project = TestProject::new();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("post-commit hook installed"));

    let hook_path = project.path().join(".git").join("hooks").join("post-commit");
    assert!(hook_path.exists());

    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("#!/bin/sh"));
    assert!(content.contains("bobbin index --quiet"));
    assert!(content.contains(">>> bobbin post-commit hook >>>"));
    assert!(content.contains("<<< bobbin post-commit hook <<<"));

    // Verify executable
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::metadata(&hook_path).unwrap().permissions();
    assert!(perms.mode() & 0o111 != 0, "Hook should be executable");
}

#[test]
fn install_git_hook_idempotent() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Install twice
    for _ in 0..2 {
        Command::new(TestProject::bobbin_bin())
            .args(["hook", "install-git-hook"])
            .current_dir(project.path())
            .assert()
            .success();
    }

    let content = std::fs::read_to_string(
        project.path().join(".git").join("hooks").join("post-commit"),
    )
    .unwrap();

    // Should only contain one bobbin section
    let marker_count = content.matches(">>> bobbin post-commit hook >>>").count();
    assert_eq!(marker_count, 1, "Should not duplicate bobbin section");
}

#[test]
fn install_git_hook_appends_to_existing() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Write an existing hook
    let hooks_dir = project.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("post-commit"),
        "#!/bin/sh\necho 'existing hook'\n",
    )
    .unwrap();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(hooks_dir.join("post-commit")).unwrap();
    assert!(content.contains("echo 'existing hook'"), "Original hook preserved");
    assert!(content.contains("bobbin index --quiet"), "Bobbin hook added");
}

// --- bobbin hook uninstall-git-hook ---

#[test]
fn uninstall_git_hook_removes_section() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Install then uninstall
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall-git-hook"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hook removed"));

    // Hook file should be removed (was only bobbin)
    let hook_path = project.path().join(".git").join("hooks").join("post-commit");
    assert!(!hook_path.exists(), "Hook file should be removed when empty");
}

#[test]
fn uninstall_git_hook_preserves_other_sections() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Write existing hook, then install bobbin
    let hooks_dir = project.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("post-commit"),
        "#!/bin/sh\necho 'existing hook'\n",
    )
    .unwrap();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    // Uninstall bobbin
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(hooks_dir.join("post-commit")).unwrap();
    assert!(content.contains("echo 'existing hook'"), "Original hook preserved");
    assert!(!content.contains("bobbin"), "Bobbin section removed");
}

#[test]
fn uninstall_git_hook_no_hook_file() {
    let project = TestProject::new();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "uninstall-git-hook"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No post-commit hook found"));
}

// --- bobbin hook status ---

#[test]
fn hook_status_shows_installed_state() {
    let project = TestProject::new();
    project.git_commit("initial");

    // Initialize bobbin (status needs .bobbin/config.toml)
    Command::new(TestProject::bobbin_bin())
        .args(["init"])
        .current_dir(project.path())
        .assert()
        .success();

    // Install hooks
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install-git-hook"])
        .current_dir(project.path())
        .assert()
        .success();

    // Check status
    Command::new(TestProject::bobbin_bin())
        .args(["hook", "status"])
        .current_dir(project.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("installed")
                .and(predicate::str::contains("Threshold"))
                .and(predicate::str::contains("Budget")),
        );
}

#[test]
fn hook_status_json_reflects_installation() {
    let project = TestProject::new();
    project.git_commit("initial");

    Command::new(TestProject::bobbin_bin())
        .args(["init"])
        .current_dir(project.path())
        .assert()
        .success();

    Command::new(TestProject::bobbin_bin())
        .args(["hook", "install"])
        .current_dir(project.path())
        .assert()
        .success();

    let output = Command::new(TestProject::bobbin_bin())
        .args(["hook", "status", "--json"])
        .current_dir(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["hooks_installed"].as_bool().unwrap(), true);
    assert_eq!(json["git_hook_installed"].as_bool().unwrap(), false);
}
