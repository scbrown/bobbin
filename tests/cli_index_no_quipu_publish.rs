//! `bobbin index --no-quipu-publish` keeps a run's chunks out of Quipu even
//! when the config publishes (aegis-1v555n.1). A mounted document share is
//! indexed for search without putting its text into the knowledge graph, and
//! the IaC makes publication opt-in by passing this flag unless a source
//! declares otherwise. The control proves the same setup DOES publish.
#![cfg(feature = "knowledge")]

mod common;

use common::TestProject;

fn project() -> TestProject {
    let project = TestProject::new();
    project.write_rust_fixtures();
    project.git_commit("initial");
    project.bobbin_init();
    let config = project.path().join(".bobbin/config.toml");
    let body = std::fs::read_to_string(&config).unwrap();
    // `bobbin init` writes the key as false; flip it (a second copy would be a
    // duplicate TOML key).
    assert!(body.contains("quipu_push_chunks = false"), "{body}");
    let body = body.replace("quipu_push_chunks = false", "quipu_push_chunks = true");
    std::fs::write(&config, body).unwrap();
    project
}

fn index(project: &TestProject, extra: &[&str]) -> String {
    let output = TestProject::bobbin_cmd()
        .arg("index")
        .args(extra)
        .arg(project.path())
        .output()
        .expect("failed to run bobbin index");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "bobbin index failed:\n{text}");
    text
}

fn published(text: &str) -> bool {
    text.contains("Published chunk snapshot")
}

#[test]
fn control_a_publishing_config_publishes() {
    let project = project();
    let text = index(&project, &[]);
    assert!(published(&text), "the control must publish:\n{text}");
}

#[test]
fn no_quipu_publish_keeps_the_run_out_of_quipu() {
    let project = project();
    let before: Vec<_> = walk(project.path());
    let text = index(&project, &["--no-quipu-publish"]);
    assert!(
        !published(&text),
        "published despite --no-quipu-publish:\n{text}"
    );
    let created: Vec<_> = walk(project.path())
        .into_iter()
        .filter(|p| !before.contains(p) && p.to_string_lossy().contains("quipu"))
        .collect();
    assert!(created.is_empty(), "a quipu store was created: {created:?}");
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}
