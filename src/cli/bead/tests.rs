//! Tests for `bead`.
//!
//! Split out of `bead.rs` so that file clears the 500-line error limit
//! (bobbin-aoz). `scripts/check-file-size.sh` exempts `*tests.rs` by
//! design, and the alternative — an allowlist entry — is the exit that
//! makes the ratchet meaningless.

use crate::storage::sqlite::PriorTouch;

use super::autolink::*;
use super::causality::*;
use super::*;

#[test]
fn test_parse_numstat_basic() {
    let out = "10\t2\tsrc/a.rs\n5\t0\tsrc/b.rs\n";
    let (files, added, deleted) = parse_numstat(out);
    assert_eq!(files, vec!["src/a.rs", "src/b.rs"]);
    assert_eq!(added, 15);
    assert_eq!(deleted, 2);
}

#[test]
fn test_parse_numstat_binary_counts_zero() {
    // Binary files emit '-' for both columns and must contribute 0.
    let out = "-\t-\tassets/logo.png\n3\t1\tsrc/a.rs\n";
    let (files, added, deleted) = parse_numstat(out);
    assert_eq!(files, vec!["assets/logo.png", "src/a.rs"]);
    assert_eq!(added, 3);
    assert_eq!(deleted, 1);
}

#[test]
fn test_parse_numstat_empty() {
    let (files, added, deleted) = parse_numstat("\n\n");
    assert!(files.is_empty());
    assert_eq!(added, 0);
    assert_eq!(deleted, 0);
}

#[test]
fn test_find_bead_id_basic() {
    assert_eq!(find_bead_id("fix bo-5em9 now"), Some("bo-5em9".to_string()));
    assert_eq!(find_bead_id("aegis-abc123"), Some("aegis-abc123".to_string()));
    // Suffix must be >= 3 chars.
    assert_eq!(find_bead_id("a-bc"), None);
    // No dash → not a bead id.
    assert_eq!(find_bead_id("hello world"), None);
    // Suffix stops at the next dash (branch-style names).
    assert_eq!(
        find_bead_id("bo-5em9-autolink-hook"),
        Some("bo-5em9".to_string())
    );
}

#[test]
fn test_find_bead_id_requires_lowercase_prefix() {
    // Uppercase prefix is not a bead id.
    assert_eq!(find_bead_id("BO-5EM9"), None);
    // Digit-led token is not a bead id (no lowercase prefix).
    assert_eq!(find_bead_id("123-abc"), None);
}

#[test]
fn test_parenthesized_bead_id() {
    assert_eq!(
        parenthesized_bead_id("feat(config): surface knobs (bo-qlfu)"),
        Some("bo-qlfu".to_string())
    );
    // The conventional-commit scope `(config)` is not a bead id; the trailing
    // `(bo-qlfu)` is.
    assert_eq!(parenthesized_bead_id("chore: tidy (cleanup)"), None);
}

#[test]
fn test_extract_bead_id_trailer_wins() {
    let msg = "feat(x): do a thing (bo-aaa111)\n\nBody.\n\nBead: bo-zzz999\n";
    assert_eq!(extract_bead_id(msg, None), Some("bo-zzz999".to_string()));
}

#[test]
fn test_extract_bead_id_paren_over_plain() {
    // Subject has both a plain token and a parenthesized one; paren wins.
    let msg = "bo-aaa111 relates to feature (bo-bbb222)";
    assert_eq!(extract_bead_id(msg, None), Some("bo-bbb222".to_string()));
}

#[test]
fn test_extract_bead_id_subject_token() {
    let msg = "wip on bo-ccc333 stuff";
    assert_eq!(extract_bead_id(msg, None), Some("bo-ccc333".to_string()));
}

#[test]
fn test_extract_bead_id_branch_fallback() {
    // No bead id in the message → fall back to the branch name.
    let msg = "wip: misc cleanup";
    assert_eq!(
        extract_bead_id(msg, Some("bo-5em9-autolink-hook")),
        Some("bo-5em9".to_string())
    );
}

#[test]
fn test_extract_bead_id_none() {
    assert_eq!(extract_bead_id("just a normal commit", None), None);
    assert_eq!(extract_bead_id("just a normal commit", Some("main")), None);
}

fn touch(bead: &str, sha: &str, file: &str, at: &str) -> PriorTouch {
    PriorTouch {
        bead_id: bead.to_string(),
        commit_sha: Some(sha.to_string()),
        file: file.to_string(),
        created_at: at.to_string(),
    }
}

#[test]
fn test_reconstruct_culprits_picks_most_recent() {
    let fix_files = vec!["src/a.rs".to_string()];
    // Two prior commits touched a.rs; the more recent (DESC-first) wins.
    let prior = vec![
        touch("bo-new", "sha_new", "src/a.rs", "2026-06-20T00:00:00Z"),
        touch("bo-old", "sha_old", "src/a.rs", "2026-06-01T00:00:00Z"),
    ];
    let got = reconstruct_culprits(&fix_files, &prior);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].culprit_sha, "sha_new");
    assert_eq!(got[0].culprit_bead_id, "bo-new");
    // Single fix file fully overlapped → max confidence.
    assert!((got[0].confidence - 0.95).abs() < 1e-9);
}

#[test]
fn test_reconstruct_culprits_confidence_scales_with_overlap() {
    let fix_files = vec![
        "src/a.rs".to_string(),
        "src/b.rs".to_string(),
        "src/c.rs".to_string(),
        "src/d.rs".to_string(),
    ];
    // sha_wide touched 2 of the 4 fix files; sha_narrow touched only 1.
    let prior = vec![
        touch("bo-wide", "sha_wide", "src/a.rs", "2026-06-10T00:00:00Z"),
        touch("bo-wide", "sha_wide", "src/b.rs", "2026-06-10T00:00:00Z"),
        touch("bo-narrow", "sha_narrow", "src/c.rs", "2026-06-09T00:00:00Z"),
    ];
    let got = reconstruct_culprits(&fix_files, &prior);
    // a.rs and b.rs → sha_wide (2/4 = 0.5); c.rs → sha_narrow (1/4 = 0.25).
    let a = got.iter().find(|c| c.file == "src/a.rs").unwrap();
    let c = got.iter().find(|c| c.file == "src/c.rs").unwrap();
    assert_eq!(a.culprit_sha, "sha_wide");
    assert!((a.confidence - 0.5).abs() < 1e-9);
    assert!((c.confidence - 0.25).abs() < 1e-9);
    // Sorted by confidence desc → wider-blame culprit first.
    assert!(got[0].confidence >= got[got.len() - 1].confidence);
}

#[test]
fn test_reconstruct_culprits_ignores_unrelated_files_and_empty_sha() {
    let fix_files = vec!["src/a.rs".to_string()];
    let prior = vec![
        // Touches a file the fix didn't → ignored.
        touch("bo-x", "sha_x", "src/other.rs", "2026-06-10T00:00:00Z"),
        // Empty sha → skipped.
        PriorTouch {
            bead_id: "bo-y".to_string(),
            commit_sha: None,
            file: "src/a.rs".to_string(),
            created_at: "2026-06-11T00:00:00Z".to_string(),
        },
    ];
    assert!(reconstruct_culprits(&fix_files, &prior).is_empty());
}

fn cand(file: &str, sha: &str, bead: &str, conf: f64) -> CausalityCandidate {
    CausalityCandidate {
        file: file.to_string(),
        culprit_sha: sha.to_string(),
        culprit_bead_id: bead.to_string(),
        confidence: conf,
    }
}

#[test]
fn test_dominant_culprit_majority_and_ties() {
    let shas = vec![
        "aaa".to_string(),
        "bbb".to_string(),
        "aaa".to_string(),
        "aaa".to_string(),
    ];
    assert_eq!(dominant_culprit(&shas), Some(("aaa".to_string(), 3, 4)));
    // Tie on count → lexically smallest sha wins (deterministic).
    let tie = vec!["zzz".to_string(), "mmm".to_string()];
    assert_eq!(dominant_culprit(&tie), Some(("mmm".to_string(), 1, 2)));
    assert_eq!(dominant_culprit(&[]), None);
}

#[test]
fn test_blame_confidence_band() {
    // Unanimous blame → top of band; split blame → lower but floored at 0.6.
    assert!((blame_confidence(10, 10) - 0.98).abs() < 1e-9);
    assert!(blame_confidence(1, 10) >= 0.6);
    assert!(blame_confidence(5, 10) > blame_confidence(1, 10));
}

#[test]
fn test_merge_causality_blame_overrides_fallback() {
    use std::collections::HashMap;
    // Fallback put sha_old on a.rs at 0.95; blame says the real culprit is
    // sha_real (unanimous), so blame replaces it with a blame-band score.
    let fallback = vec![
        cand("src/a.rs", "sha_old", "bo-old", 0.95),
        cand("src/b.rs", "sha_b", "bo-b", 0.4),
    ];
    let mut blame: HashMap<String, Vec<String>> = HashMap::new();
    blame.insert(
        "src/a.rs".to_string(),
        vec!["sha_real".to_string(), "sha_real".to_string()],
    );
    let merged = merge_causality(fallback, &blame);

    let a = merged.iter().find(|c| c.file == "src/a.rs").unwrap();
    assert_eq!(a.culprit_sha, "sha_real");
    assert_eq!(a.culprit_bead_id, ""); // blame doesn't know the bead
    assert!((a.confidence - 0.98).abs() < 1e-9);

    // b.rs had no blame → fallback survives untouched.
    let b = merged.iter().find(|c| c.file == "src/b.rs").unwrap();
    assert_eq!(b.culprit_sha, "sha_b");
    assert_eq!(b.culprit_bead_id, "bo-b");
}

#[test]
fn test_merge_causality_adds_blame_only_files() {
    use std::collections::HashMap;
    // A file blame found but with no fallback candidate is still recorded.
    let mut blame: HashMap<String, Vec<String>> = HashMap::new();
    blame.insert("src/new.rs".to_string(), vec!["sha_intro".to_string()]);
    let merged = merge_causality(vec![], &blame);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].file, "src/new.rs");
    assert_eq!(merged[0].culprit_sha, "sha_intro");
}
