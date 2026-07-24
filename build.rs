use std::process::Command;

// Capture the git sha + dirty flag at BUILD time so the running binary can report
// exactly which commit it was built from. This is what makes a deploy
// verifiable: the CD driver's strongest health check is "the running service reports
// the sha we just built", and without it a deploy has to be declared successful on
// the strength of a 200 — the "green and wrong" outcome the pipeline exists to
// prevent. quipu already does this; bobbin was the gap.
//
// Absent .git (e.g. a tarball build) yields "unknown", which is honest rather than
// wrong — a probe that fabricates a sha is worse than one that admits it cannot tell.
fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    println!("cargo:rustc-env=BOBBIN_GIT_SHA={sha}");
    println!("cargo:rustc-env=BOBBIN_GIT_DIRTY={dirty}");

    // Rebuild when HEAD (or the index, for the dirty flag) moves, so the embedded
    // sha never goes stale under an incremental build.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
