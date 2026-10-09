//! Legacy Latin-1 files are decoded and indexed in both full and incremental
//! runs. Before the decoding fallback they were skipped and counted as errors.

mod common;

use common::TestProject;

/// Latin-1 `é` (0xE9) is not valid UTF-8, the shape found on a real NAS.
const LATIN1: &[u8] = b"pub fn caf\xe9() -> i32 { 1 }\n";

fn index_json(project: &TestProject, force: bool) -> serde_json::Value {
    let mut cmd = TestProject::bobbin_cmd();
    cmd.args(["--json", "index", "--skip-calibrate"]);
    if force {
        cmd.arg("--force");
    }
    let output = cmd
        .arg(project.path())
        .output()
        .expect("failed to run bobbin index");
    assert!(
        output.status.success(),
        "`bobbin index` must not fail on one legacy file (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("index --json output")
}

fn project_with_legacy_file() -> TestProject {
    let project = TestProject::new();
    project.write_rust_fixtures();
    std::fs::write(project.path().join("src_latin1.rs"), LATIN1).unwrap();
    project.git_commit("initial");
    project.bobbin_init();
    project
}

#[test]
fn a_first_incremental_index_decodes_a_legacy_file() {
    let project = project_with_legacy_file();
    let json = index_json(&project, false);
    assert_eq!(json["errors"], 0, "{json}");
    assert!(json["files_indexed"].as_u64().unwrap() >= 3, "{json}");
}

#[test]
fn incremental_and_force_decode_legacy_text() {
    // Both paths must decode identically so the incremental content hash is stable.
    let project = project_with_legacy_file();
    let forced = index_json(&project, true);
    let incremental = index_json(&project, false);
    assert_eq!(forced["errors"], 0, "{forced}");
    assert_eq!(incremental["errors"], 0, "{incremental}");
    assert_eq!(
        incremental["files_indexed"], 0,
        "unchanged decoded files must be skipped: {incremental}"
    );
}

#[test]
fn a_legacy_file_added_later_does_not_stop_the_next_run() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();
    assert!(
        index_json(&project, false)["files_indexed"]
            .as_u64()
            .unwrap()
            >= 2
    );

    std::fs::write(project.path().join("src_latin1.rs"), LATIN1).unwrap();
    project.write_file("src_added.rs", "pub fn added() -> i32 { 2 }\n");
    let json = index_json(&project, false);
    assert_eq!(json["errors"], 0, "{json}");
    assert!(
        json["files_indexed"].as_u64().unwrap() >= 2,
        "the readable new file still lands: {json}"
    );
}
#[cfg(unix)]
#[test]
fn actual_read_error_is_still_skipped_and_counted() {
    use std::os::unix::fs::PermissionsExt;
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.write_file("unreadable.rs", "pub fn inaccessible() {}\n");
    project.git_commit("initial");
    project.bobbin_init();
    let path = project.path().join("unreadable.rs");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0)).unwrap();
    let json = index_json(&project, true);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(json["errors"], 1, "{json}");
    assert!(json["files_indexed"].as_u64().unwrap() >= 2, "{json}");
}
