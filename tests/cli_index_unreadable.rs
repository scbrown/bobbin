//! An unreadable file is reported and skipped, never a reason to abort the run
//! (aegis-1v555n.1). Before the fix the incremental pre-pass propagated the read
//! error, so one legacy Latin-1 file stopped every incremental index of its
//! source, while `--force` over the same tree skipped it and carried on.

mod common;

use common::TestProject;

/// Latin-1 `é` (0xE9) is not valid UTF-8, the shape found on a real NAS.
const LATIN1: &[u8] = b"pub fn caf\xe9() -> i32 { 1 }\n";

fn index_json(project: &TestProject, force: bool) -> serde_json::Value {
    let mut cmd = TestProject::bobbin_cmd();
    cmd.args(["--json", "index"]);
    if force {
        cmd.arg("--force");
    }
    let output = cmd
        .arg(project.path())
        .output()
        .expect("failed to run bobbin index");
    assert!(
        output.status.success(),
        "`bobbin index` must not fail on one unreadable file (status {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("index --json output")
}

fn project_with_unreadable_file() -> TestProject {
    let project = TestProject::new();
    project.write_rust_fixtures();
    std::fs::write(project.path().join("src_latin1.rs"), LATIN1).unwrap();
    project.git_commit("initial");
    project.bobbin_init();
    project
}

#[test]
fn a_first_incremental_index_skips_an_unreadable_file_and_counts_it() {
    let project = project_with_unreadable_file();
    let json = index_json(&project, false);
    assert_eq!(json["errors"], 1, "{json}");
    assert!(json["files_indexed"].as_u64().unwrap() >= 2, "{json}");
}

#[test]
fn incremental_and_force_report_the_same_error() {
    // CONTROL: --force never took the aborting path, so parity with it is the
    // property this fix restores.
    let project = project_with_unreadable_file();
    let forced = index_json(&project, true);
    let incremental = index_json(&project, false);
    assert_eq!(forced["errors"], 1, "{forced}");
    assert_eq!(incremental["errors"], 1, "{incremental}");
}

#[test]
fn an_unreadable_file_added_later_does_not_stop_the_next_run() {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();
    project.index_or_explain();

    std::fs::write(project.path().join("src_latin1.rs"), LATIN1).unwrap();
    project.write_file("src_added.rs", "pub fn added() -> i32 { 2 }\n");
    let json = index_json(&project, false);
    assert_eq!(json["errors"], 1, "{json}");
    assert!(
        json["files_indexed"].as_u64().unwrap() >= 1,
        "the readable new file still lands: {json}"
    );
}
