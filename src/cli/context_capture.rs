//! Explicit, private diagnostic output. Never collected by the production hook.
use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::search::context::ContextBundle;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
}

pub(super) fn write(
    path: &Path,
    bundle: &ContextBundle,
    root: &Path,
    model: &str,
    role: &str,
) -> Result<()> {
    let mut executable = std::fs::File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut executable, &mut hash)?;
    let artifact = json!({
        "schema": "bobbin-assembly-capture-v1",
        "scope": "local-context-assembly-only",
        "query": bundle.query,
        "provenance": {
            "binary_sha256": hex::encode(hash.finalize()),
            "binary_source_commit": env!("BOBBIN_GIT_SHA"),
            "binary_source_dirty": env!("BOBBIN_GIT_DIRTY"),
            "checkout_commit": git(root, &["rev-parse", "HEAD"]),
            "checkout_dirty": git(root, &["status", "--porcelain"]).map(|s| !s.is_empty()),
            "embedding_model": model, "role": role,
            "index_snapshot": "not_pinned",
        },
        "capture": bundle.capture.as_ref().context("Assembly capture missing")?,
        "baseline": bundle,
        "hook_gate": "not_run", "session_dedup": "not_run",
        "replay_ready": false,
        "limitations": [
            "Baseline is the same assembly call, not the rendered hook output or an ArmScore.",
            "No top-30 selection, model judgment, labels, or scoring has occurred.",
            "Index bytes and hook state must be pinned before an experiment.",
            "Candidates may occur in multiple legs; scores have leg-specific meanings.",
            "Role filtering matches the local context command; this is not the remote hook path."
        ],
    });
    persist(path, &serde_json::to_vec_pretty(&artifact)?)
}

fn persist(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // NamedTempFile is private (0600 on Unix). No shared log, partial output, or overwrite.
    let mut stage = tempfile::NamedTempFile::new_in(parent)?;
    stage.write_all(bytes)?;
    stage.write_all(b"\n")?;
    stage.as_file().sync_all()?;
    stage.persist_noclobber(path).with_context(|| {
        format!(
            "Capture output already exists or cannot be created: {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_refuses_overwrite_and_is_private() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("capture.json");
        persist(&path, b"first").unwrap();
        assert!(persist(&path, b"second").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
