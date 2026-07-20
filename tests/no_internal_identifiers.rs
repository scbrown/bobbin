//! THE RATCHET: no internal identifier gets back into this public repo.
//!
//! A scrub is a STATE and states rot. This repo was scrubbed of 41 internal
//! hostnames and paths; without a mechanism, number 42 arrives within days from
//! someone writing a perfectly good doc. The deliverable was never the 41
//! strings — it is this test.
//!
//! IT SCANS BYTES, NOT JUST TEXT FILES, AND THAT IS THE WHOLE POINT.
//! `data/quipu.db` sat tracked in this repo carrying ~2.1MB of a live private
//! graph — several thousand internal service and host names — and the sweep
//! written to find exactly that missed it, because scanners skip binaries by
//! suffix. The file was invisible to the tool designed to find it. So this reads
//! every tracked file as raw bytes and matches the lossy decode: a database, a
//! log or a compiled blob cannot hide from it.
//!
//! NO LOOKAROUNDS. Rust's `regex` is RE2-flavoured and has none, which is also
//! why the graph rule's patterns are written RE2-compatible: the same pattern
//! strings have to run in a Rust consumer and a Python one. Where a negative
//! condition is needed, capture and filter in code instead.
//!
//! SCOPE: an offline subset of the authoritative rule, which lives in the Quipu
//! knowledge graph. Duplicated here in part, on purpose — a public repo's test
//! suite must name no internal service and must pass with no network. To change
//! WHAT is forbidden, change the graph rule, not this file.

use std::path::PathBuf;
use std::process::Command;

/// Conventional placeholder accounts. These are the documented FIX, so flagging
/// them would make the guard fire on its own advice — and a guard that does that
/// is one the next person deletes.
const PLACEHOLDER_ACCOUNTS: &[&str] = &["user", "you", "alice", "bob", "example", "someone"];

fn patterns() -> Vec<(&'static str, regex::Regex)> {
    vec![
        (
            "internal hostname",
            regex::Regex::new(r"\b[a-z0-9][a-z0-9-]*\.(?:lan|svc)\b").unwrap(),
        ),
        (
            "private address",
            regex::Regex::new(
                r"\b(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b",
            )
            .unwrap(),
        ),
        (
            "operator home path",
            regex::Regex::new(r"/home/([a-z][a-z0-9_-]*)/").unwrap(),
        ),
        (
            // Bare place-name hosts, no .lan/.svc TLD. Word-anchored, and that is
            // measured not stylistic: unanchored, one of these matches ordinary
            // English (derivative, activation) for dozens of false positives —
            // the cry-wolf that gets a guard deleted.
            "internal node name",
            regex::Regex::new(r"\b(kota|luvu|vati|koror|palau|yap)\b").unwrap(),
        ),
    ]
}

/// A hit is real unless it is a known placeholder. Split out so both the scan and
/// its controls apply the identical rule.
fn is_real_hit(label: &str, caps: &regex::Captures) -> bool {
    if label == "operator home path" {
        let account = caps.get(1).map_or("", |m| m.as_str());
        return !PLACEHOLDER_ACCOUNTS.contains(&account);
    }
    true
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tracked_files() -> Vec<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root())
        .arg("ls-files")
        .output()
        .expect("git ls-files");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| root().join(l))
        .collect()
}

#[test]
fn no_internal_identifiers_in_any_tracked_file() {
    let pats = patterns();
    let me = root().join("tests/no_internal_identifiers.rs");
    let mut offenders: Vec<String> = Vec::new();

    for path in tracked_files() {
        if path == me {
            continue; // this file names the patterns; that is its job
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for (label, rx) in &pats {
            if let Some(caps) = rx.captures_iter(&text).find(|c| is_real_hit(label, c)) {
                let rel = path.strip_prefix(root()).unwrap_or(&path);
                offenders.push(format!(
                    "{}: {} {:?}",
                    rel.display(),
                    label,
                    caps.get(0).unwrap().as_str()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "internal identifier(s) in a PUBLIC repo:\n  {}\n\nUse a neutral example \
         (RFC 2606 reserves .example/.invalid for exactly this) or a placeholder \
         account. Never commit a runtime store — see data/ in .gitignore.",
        offenders.join("\n  ")
    );
}

#[test]
fn the_ratchet_catches_each_class() {
    // Positive control, one per class. A guard never seen catching anything is a
    // function returning an empty vector, and it looks exactly like a clean repo.
    let pats = patterns();
    for (expect, sample) in [
        ("internal hostname", "connect to db.lan now"),
        ("internal hostname", "http://thing.svc/mcp"),
        ("private address", "addr 192.168.0.1"),
        ("operator home path", "/home/jsmith/src/x"),
        ("internal node name", "rebuilt on koror overnight"),
    ] {
        let caught = pats.iter().any(|(label, rx)| {
            *label == expect && rx.captures_iter(sample).any(|c| is_real_hit(label, &c))
        });
        assert!(caught, "the ratchet missed a planted {expect}: {sample:?}");
    }
}

#[test]
fn placeholders_and_public_addresses_are_allowed() {
    // The negative control. These are what we TELL people to write.
    let pats = patterns();
    for ok in [
        "/home/user/src/x",
        "/home/alice/src/x",
        "host.example",
        "8.8.8.8",
        "the derivative activation of a motivation",
    ] {
        for (label, rx) in &pats {
            let hit = rx.captures_iter(ok).any(|c| is_real_hit(label, &c));
            assert!(!hit, "{label} wrongly flagged an allowed sample: {ok:?}");
        }
    }
}
