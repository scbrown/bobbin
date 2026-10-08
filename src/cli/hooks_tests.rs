use super::*;

#[test]
fn the_bundle_is_valid_owned_by_bobbin_and_versioned_by_the_build() {
    let b = bundle();
    assert_eq!(b["schema"], "st.hook-bundle/1");
    assert_eq!(b["name"], "bobbin");
    assert_eq!(b["owner"], "bobbin");
    assert_eq!(b["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn a_hook_without_harnesses_applies_to_both() {
    let b = bundle();
    let claude = hooks_for(&b, Harness::Claude);
    let codex = hooks_for(&b, Harness::Codex);
    assert!(claude.len() > codex.len(), "claude-only hooks exist");
    for hook in &codex {
        assert!(claude.contains(hook), "{hook:?} is codex-only");
    }
    assert!(codex.iter().any(|h| h.0 == "UserPromptSubmit"));
}

#[test]
fn merge_is_idempotent_and_keeps_foreign_hooks() {
    let b = bundle();
    let mut cfg = json!({"hooks": {"UserPromptSubmit": [
        {"hooks": [{"type": "command", "command": "other-tool hook"}]}
    ]}, "model": "x"});
    let want = hooks_for(&b, Harness::Claude).len();
    assert_eq!(merge(&mut cfg, &b, Harness::Claude), want);
    assert_eq!(merge(&mut cfg, &b, Harness::Claude), 0);
    assert_eq!(present(&cfg, &b, Harness::Claude), (want, want));
    assert_eq!(cfg["model"], "x");
    let ups = &cfg["hooks"]["UserPromptSubmit"][0]["hooks"];
    assert!(ups
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["command"] == "other-tool hook"));
}

#[test]
fn remove_takes_only_bobbins_hooks() {
    let b = bundle();
    let mut cfg = json!({"hooks": {"UserPromptSubmit": [
        {"hooks": [{"type": "command", "command": "other-tool hook"}]}
    ]}});
    merge(&mut cfg, &b, Harness::Claude);
    let n = hooks_for(&b, Harness::Claude).len();
    assert_eq!(remove(&mut cfg, &b, Harness::Claude), n);
    assert_eq!(present(&cfg, &b, Harness::Claude).0, 0);
    assert_eq!(
        cfg["hooks"]["UserPromptSubmit"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(cfg["hooks"].get("PostToolUseFailure").is_none());
}

#[test]
fn codex_config_round_trips_through_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "model = \"gpt\"\n[mcp_servers.x]\ncommand = \"x\"\n").unwrap();
    let b = bundle();
    let mut cfg = read_config(&path, Harness::Codex).unwrap();
    assert!(merge(&mut cfg, &b, Harness::Codex) > 0);
    write_config(&path, Harness::Codex, &cfg).unwrap();
    let back = read_config(&path, Harness::Codex).unwrap();
    let n = hooks_for(&b, Harness::Codex).len();
    assert_eq!(present(&back, &b, Harness::Codex), (n, n));
    assert_eq!(back["model"], "gpt");
    assert_eq!(back["mcp_servers"]["x"]["command"], "x");
    assert!(dir.path().join("config.toml.bak-bobbin").exists());
}
