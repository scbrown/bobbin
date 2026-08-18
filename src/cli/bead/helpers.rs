//! Shared bead/git helpers used by more than one `bobbin bead` subcommand.
//!
//! Split out of the former `src/cli/bead.rs` (bobbin-aoz).

use std::path::Path;
use std::process::Command;

use crate::index::Parser;
use crate::storage::sqlite::TouchedSymbol;

/// Fetch a bead as JSON via `bd show <id> --json`. bd may emit a single object
/// or a one-element array; this normalizes to the first object. Best-effort:
/// returns None on any failure so lineage recording never hard-fails on
/// telemetry enrichment.
pub(super) fn bead_json(bead_id: &str) -> Option<serde_json::Value> {
    let out = Command::new("bd")
        .args(["show", bead_id, "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    match v {
        serde_json::Value::Array(arr) => arr.into_iter().next(),
        other => Some(other),
    }
}

/// Derive bundle slugs from the bead's `b:<slug>` labels (edge E2). Best-effort:
/// returns None if bd is unavailable or no bundle labels are present.
pub(super) fn bundle_slugs_from_labels(bead_id: &str) -> Option<String> {
    let v = bead_json(bead_id)?;
    let labels = v.get("labels")?.as_array()?;
    let slugs: Vec<String> = labels
        .iter()
        .filter_map(|l| l.as_str())
        .filter_map(|l| l.strip_prefix("b:"))
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if slugs.is_empty() {
        None
    } else {
        Some(slugs.join(","))
    }
}

/// Resolve the feature ancestor of a bead by walking its dependency graph (edge
/// E1 'implements'). Returns the id of the first `feature`-typed ancestor found,
/// or None. Best-effort: cycle-guarded (visited set), depth-capped at 10, and
/// NULL on any bd failure.
pub(super) fn resolve_feature_id(bead_id: &str) -> Option<String> {
    use std::collections::HashSet;
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = vec![bead_id.to_string()];
    let mut depth = 0;
    while !frontier.is_empty() && depth < 10 {
        let mut next: Vec<String> = Vec::new();
        for id in frontier {
            if !visited.insert(id.clone()) {
                continue;
            }
            let v = match bead_json(&id) {
                Some(v) => v,
                None => continue,
            };
            if let Some(deps) = v.get("dependencies").and_then(|d| d.as_array()) {
                for dep in deps {
                    let dep_id = match dep.get("id").and_then(|i| i.as_str()) {
                        Some(i) => i,
                        None => continue,
                    };
                    let dep_type = dep.get("issue_type").and_then(|t| t.as_str()).unwrap_or("");
                    if dep_type == "feature" {
                        return Some(dep_id.to_string());
                    }
                    next.push(dep_id.to_string());
                }
            }
        }
        frontier = next;
        depth += 1;
    }
    None
}

/// Best-effort symbol extraction for a commit's changed files. For each file we
/// parse its committed version (`git show <sha>:<path>`) and collect named
/// chunks. Binary / unparseable / deleted files are skipped silently.
pub(super) fn extract_touched_symbols(
    repo_root: &Path,
    sha: &str,
    files: &[String],
) -> Vec<TouchedSymbol> {
    let mut parser = match Parser::new() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for file in files {
        let blob = Command::new("git")
            .current_dir(repo_root)
            .args(["show", &format!("{}:{}", sha, file)])
            .output();
        let content = match blob {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => continue, // deleted / binary / missing at this revision
        };
        let chunks = match parser.parse_file(Path::new(file), &content) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for chunk in chunks {
            if let Some(name) = chunk.name {
                out.push(TouchedSymbol {
                    file: file.clone(),
                    symbol: name,
                    kind: chunk.chunk_type.to_string(),
                });
            }
        }
    }
    out
}
