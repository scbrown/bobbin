//! Matching wires to the files in a bundle, and rendering the section.

use std::path::Path;

use super::{GovernedTripwire, Provenance};

/// At most this many wires are rendered. A boundary the agent cannot read
/// because it scrolled past 40 lines of governance is not surfaced.
/// Truncation is announced in band, never silent.
const MAX_RENDERED: usize = 6;

/// One wire and the bundle files that fall inside its boundary.
#[derive(Debug, Clone)]
pub struct Match<'a> {
    /// The governed wire.
    pub wire: &'a GovernedTripwire,
    /// `(repo, repo-relative path)` for each file inside the boundary.
    /// `repo` is `None` when the path gave no repo to name.
    pub files: Vec<(Option<String>, String)>,
}

/// Split an indexed path into `(repo, repo-relative path)`.
///
/// `aegis:appliesTo` globs are **repo-relative** by quipu's definition, but
/// bobbin indexes absolute server paths, `repo/rel` relative paths and bare
/// relative paths in the same table. Matching a repo-relative glob against an
/// absolute path silently matches nothing, which looks exactly like "no wire
/// spans this file" — the failure mode this whole module exists to avoid.
///
/// Only the three shapes bobbin actually produces are handled, and no
/// speculative suffix-stripping is done: inventing a repo root to make a glob
/// fit would manufacture boundaries that do not exist.
#[must_use]
pub fn split_repo(
    path: &str,
    repo_path_prefix: Option<&str>,
    repo_root: &Path,
) -> (Option<String>, String) {
    // 1. Under the configured server prefix: `<prefix>/<repo>/<rel>`.
    if let Some(prefix) = repo_path_prefix.filter(|p| !p.is_empty()) {
        if let Some(after) = path.strip_prefix(prefix) {
            let after = after.trim_start_matches('/');
            if let Some((repo, rel)) = after.split_once('/') {
                return (Some(repo.to_string()), rel.to_string());
            }
        }
    }
    // 2. Absolute and under this repo root.
    if let Ok(rel) = Path::new(path).strip_prefix(repo_root) {
        let repo = repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string());
        return (repo, rel.to_string_lossy().to_string());
    }
    // 3. Already relative. No repo to name; the glob is matched as given.
    (None, path.trim_start_matches("./").to_string())
}

/// Every wire spanning at least one file in `paths`.
///
/// Order is the projection's order, so a stable quipu catalog renders a stable
/// section and an agent is not re-reading a reshuffled list every turn.
#[must_use]
pub fn matching<'a>(
    wires: &'a [GovernedTripwire],
    paths: &[String],
    repo_path_prefix: Option<&str>,
    repo_root: &Path,
) -> Vec<Match<'a>> {
    let split: Vec<(Option<String>, String)> = paths
        .iter()
        .map(|p| split_repo(p, repo_path_prefix, repo_root))
        .collect();

    let mut out = Vec::new();
    for wire in wires {
        let patterns: Vec<glob::Pattern> = wire
            .paths
            .iter()
            .filter_map(|g| glob::Pattern::new(g).ok())
            .collect();
        let mut files: Vec<(Option<String>, String)> = Vec::new();
        for (repo, rel) in &split {
            if patterns.iter().any(|p| p.matches(rel)) && !files.iter().any(|(_, r)| r == rel) {
                files.push((repo.clone(), rel.clone()));
            }
        }
        if !files.is_empty() {
            out.push(Match { wire, files });
        }
    }
    out
}

/// Render the governance section, or `None` when there is nothing honest to
/// say.
///
/// Returns `None` when no wire spans any file in the bundle **and** the
/// projection is current. When the projection is stale because a refresh
/// failed, a short note is emitted even with no matches: "I could not look" is
/// a different fact from "I looked and there was nothing", and an agent
/// deserves to be able to tell them apart. That is the same discipline the
/// hook's own expand-injection path already states in `src/cli/hook.rs`.
#[must_use]
pub fn section(
    matches: &[Match<'_>],
    provenance: &Provenance,
    format_mode: &str,
) -> Option<String> {
    let refresh_failed = matches!(
        provenance,
        Provenance::Cached {
            refresh_error: Some(_),
            ..
        }
    );
    if matches.is_empty() && !refresh_failed {
        return None;
    }
    let xml = format_mode == "xml";
    let mut out = String::new();

    if matches.is_empty() {
        // Stale, and nothing matched — say only that, and say why.
        let body = format!(
            "No governed path boundary matched these files, but the policy \
             projection could not be refreshed, so this is last-known and not \
             current. {}",
            provenance.note()
        );
        return Some(wrap(&mut out, xml, &body));
    }

    push(
        &mut out,
        "Governed path boundaries (tripwires) spanning files in this context:",
    );
    push(&mut out, "");
    let multi_repo = matches
        .iter()
        .flat_map(|m| m.files.iter())
        .filter_map(|(r, _)| r.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;

    for m in matches.iter().take(MAX_RENDERED) {
        let w = m.wire;
        push(
            &mut out,
            &format!("- {} — effect: {}", w.name, w.effect.as_str()),
        );
        if let Some(claim) = &w.claim {
            push(&mut out, &format!("    claim: {claim}"));
        }
        push(&mut out, &format!("    boundary: {}", w.paths.join(", ")));
        let files: Vec<String> = m
            .files
            .iter()
            .map(|(repo, rel)| match repo {
                Some(r) => format!("{r}:{rel}"),
                None => rel.clone(),
            })
            .collect();
        push(&mut out, &format!("    in context: {}", files.join(", ")));
        push(&mut out, &format!("    placement: {}", placement(w)));
        if let Some(defect) = w.defect() {
            push(&mut out, &format!("    ⚠ MALFORMED — {defect}"));
        }
    }
    if matches.len() > MAX_RENDERED {
        push(
            &mut out,
            &format!(
                "- … and {} further wire(s) spanning these files, not shown.",
                matches.len() - MAX_RENDERED
            ),
        );
    }

    push(&mut out, "");
    push(
        &mut out,
        "Bobbin does not enforce these; it reports what the governance graph declares. \
         Enforcement, where it exists, is the governing host's (yupana's pre-edit guard \
         for the wires it can enforce).",
    );
    if multi_repo {
        push(
            &mut out,
            "Note: aegis:appliesTo globs are repo-relative and carry no repo scoping, so a \
             wire matches the same relative path in every indexed repo. The repo is named \
             above so you can judge which matches were meant.",
        );
    }
    push(&mut out, &provenance.note());

    let body = out.trim_end().to_string();
    let mut wrapped = String::new();
    Some(wrap(&mut wrapped, xml, &body))
}

/// How the policy says it is judged, in words rather than acronyms.
fn placement(w: &GovernedTripwire) -> String {
    let class = w.class.as_deref().unwrap_or("unspecified class");
    match w.verification_point.as_deref() {
        Some("PAG") => format!("{class}, judged at the pre-action gate (before the edit lands)"),
        Some("PAA") => format!(
            "{class}, judged after the action — it prices a completed crossing and backs off \
             what comes next, it does not stop this edit"
        ),
        Some(other) => format!("{class}, verification point {other}"),
        None => format!("{class}, no verification point declared"),
    }
}

fn push(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn wrap(out: &mut String, xml: bool, body: &str) -> String {
    if xml {
        out.push_str("<bobbin-governance>\n");
        out.push_str(body);
        out.push_str("\n</bobbin-governance>\n\n");
    } else {
        out.push_str(body);
        out.push_str("\n\n");
    }
    out.clone()
}
