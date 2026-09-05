//! Integration tests for non-filesystem index sources on the ChunkSource
//! seam (bobbin-d5e): archives get content-hash incremental indexing with a
//! removal sweep instead of the old full-replace that re-embedded every
//! record on every run.

mod common;

use common::TestProject;

/// Run `--json index` and parse the output.
fn run_index_json(project: &TestProject) -> serde_json::Value {
    let output = TestProject::bobbin_cmd()
        .args(["--json", "index"])
        .arg(project.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

/// Rewrite the generated config's `[archive]` section to enable one archive
/// source rooted at `dir`. The default section is removed (TOML forbids a
/// duplicate table) and a fully specified one is appended.
fn enable_archive_source(project: &TestProject, dir: &std::path::Path) {
    let config_path = project.path().join(".bobbin/config.toml");
    let cfg = std::fs::read_to_string(&config_path).unwrap();
    let start = cfg
        .find("[archive]")
        .expect("generated config should contain an [archive] section");
    let after = &cfg[start..];
    let end = after
        .find("\n[")
        .map(|i| start + i + 1)
        .unwrap_or(cfg.len());
    let new_cfg = format!(
        "{}{}\n[archive]\nenabled = true\nwebhook_secret = \"\"\n\n\
         [[archive.sources]]\nname = \"fieldnotes\"\npath = \"{}\"\n\
         schema = \"field-notes\"\nname_field = \"project\"\n",
        &cfg[..start],
        &cfg[end..],
        dir.display()
    );
    std::fs::write(&config_path, new_cfg).unwrap();
}

fn archive_record(id: &str, body: &str) -> String {
    format!(
        "---\nschema: field-notes/v1\nid: {id}\n\
         timestamp: 2026-03-01T12:00:00Z\nproject: alpha\n---\n\n{body}\n"
    )
}

/// bobbin-d5e: archives run through the ChunkSource seam. A second run over an
/// unchanged archive must re-embed nothing (content-hash incremental), and a
/// record that disappears must be swept from the index — the old bespoke block
/// re-embedded everything every run and never removed anything.
#[test]
fn index_archive_incremental_and_removal_sweep() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();

    // Skip if the ONNX runtime is unavailable.
    project.index_or_explain();

    // Archive records live OUTSIDE the walked tree, like a real archive mount.
    let archive_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        archive_dir.path().join("fn-001.md"),
        archive_record("fn-001", "First deployment observation."),
    )
    .unwrap();
    std::fs::write(
        archive_dir.path().join("fn-002.md"),
        archive_record("fn-002", "Second note about the rollback."),
    )
    .unwrap();

    enable_archive_source(&project, archive_dir.path());

    // First run: both records embed.
    let json = run_index_json(&project);
    assert_eq!(json["status"], "indexed");
    assert_eq!(
        json["archive_indexed"], 2,
        "first archive pass should embed both records, got {json}"
    );

    // Second run, nothing changed anywhere: 0 source files AND 0 re-embedded
    // archive records. This also pins the early-return gate: an
    // archive-enabled run must fall through instead of fast-returning
    // "up_to_date" and skipping archives entirely.
    let json = run_index_json(&project);
    assert_eq!(json["status"], "indexed");
    assert_eq!(json["files_indexed"], 0);
    assert_eq!(
        json["archive_indexed"], 0,
        "unchanged archive must not re-embed, got {json}"
    );
    assert_eq!(json["archive_unchanged"], 2);
    assert_eq!(json["archive_removed"], 0);

    // Remove one record: the sweep must delete it from the index.
    std::fs::remove_file(archive_dir.path().join("fn-002.md")).unwrap();
    let json = run_index_json(&project);
    assert_eq!(
        json["archive_removed"], 1,
        "vanished record must be swept, got {json}"
    );
    assert_eq!(json["archive_unchanged"], 1);
    assert_eq!(json["archive_indexed"], 0);
}

// ---------------------------------------------------------------------------
// `bobbin index-bead <id>` — GH#52 Phase 4
// ---------------------------------------------------------------------------

/// Point `[beads]` at `databases` without enabling a reachable server. Enough
/// for every guard `index-bead` applies before it touches Dolt.
fn configure_beads_databases(project: &TestProject, dbs: &[&str]) {
    let config_path = project.path().join(".bobbin/config.toml");
    let cfg = std::fs::read_to_string(&config_path).unwrap();
    let list = dbs
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let new_cfg = cfg.replace("databases = []", &format!("databases = [{list}]"));
    assert_ne!(
        cfg, new_cfg,
        "generated config should have an empty [beads] databases list"
    );
    std::fs::write(&config_path, new_cfg).unwrap();
}

/// `index-bead` is a bead-only command. Unlike `bobbin index`, there is no file
/// half that could still do useful work, so an unconfigured `[beads]` section
/// must be a loud error and not a silent exit-0 — a post-write hook wired to a
/// misconfigured repo would otherwise report success forever while indexing
/// nothing.
#[test]
fn index_bead_refuses_when_no_beads_databases_are_configured() {
    let project = common::init_project();
    let output = TestProject::bobbin_cmd()
        .args(["index-bead", "bo-1"])
        .arg(project.path())
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&output);
    assert!(
        stderr.contains("No beads databases configured"),
        "unexpected stderr: {stderr}"
    );
}

/// A rig name that is not configured must be rejected by name rather than
/// quietly matching nothing — "no beads found" and "you typed the rig wrong"
/// are different answers and the operator needs the second one.
#[test]
fn index_bead_rejects_an_unknown_rig() {
    let project = common::init_project();
    configure_beads_databases(&project, &["beads_aegis"]);
    let output = TestProject::bobbin_cmd()
        .args(["index-bead", "bo-1", "--rig", "nope"])
        .arg(project.path())
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&output);
    assert!(
        stderr.contains("Unknown rig `nope`"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("aegis"),
        "the error should name the configured rigs: {stderr}"
    );
}

/// The incremental fast path needs a corpus to be incremental against. It must
/// say so before loading an embedding model — `bobbin init` creates an empty
/// vector directory, so the presence of that directory proves nothing.
#[test]
fn index_bead_refuses_before_a_first_full_index() {
    let project = common::init_project();
    configure_beads_databases(&project, &["beads_aegis"]);
    let output = TestProject::bobbin_cmd()
        .args(["index-bead", "bo-1"])
        .arg(project.path())
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&output);
    assert!(
        stderr.contains("No index yet"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("bobbin index"),
        "the error should name the command that fixes it: {stderr}"
    );
}

/// The subcommand is reachable and self-describing. Cheap, but it is the only
/// assertion that the clap wiring exists at all — the rest of the suite would
/// pass identically with the command unregistered.
#[test]
fn index_bead_is_a_registered_subcommand() {
    let output = TestProject::bobbin_cmd()
        .args(["index-bead", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains("--rig"), "missing --rig: {stdout}");
    assert!(stdout.contains("--force"), "missing --force: {stdout}");
}
