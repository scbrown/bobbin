//! Tests for `beads`.
//!
//! Split out of `beads.rs` so that file clears the 500-line error limit
//! (bobbin-aoz). `scripts/check-file-size.sh` exempts `*tests.rs` by
//! design, and the alternative — an allowlist entry — is the exit that
//! makes the ratchet meaningless.

use super::*;

#[test]
fn test_chunk_id_deterministic() {
    let input = "beads:aegis:aegis-0a9";
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let id1 = hex::encode(hasher.finalize());

    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let id2 = hex::encode(hasher.finalize());

    assert_eq!(id1, id2);
    assert_eq!(id1.len(), 64);
}

fn sample_issue() -> BeadRow {
    BeadRow {
        id: "aegis-0a9".to_string(),
        title: "Fix the widget".to_string(),
        description: "The widget is broken.".to_string(),
        status: "open".to_string(),
        priority: 1,
        assignee: Some("strider".to_string()),
        notes: "investigated already".to_string(),
        metadata: String::new(),
    }
}

#[test]
fn test_metadata_is_meaningful() {
    assert!(!metadata_is_meaningful(""));
    assert!(!metadata_is_meaningful("  {}  "));
    assert!(!metadata_is_meaningful("null"));
    assert!(metadata_is_meaningful(r#"{"source":"T1"}"#));
}

#[test]
fn test_build_bead_content_includes_metadata() {
    let mut issue = sample_issue();
    issue.metadata = r#"{"source":"guidance-T1","ref":"doc#42"}"#.to_string();
    let content = build_bead_content(&issue, &[], &[]);
    assert!(content.contains("Metadata:"));
    assert!(content.contains("guidance-T1"));
}

#[test]
fn test_build_bead_content_skips_empty_metadata() {
    let issue = sample_issue(); // metadata = ""
    let content = build_bead_content(&issue, &[], &[]);
    assert!(!content.contains("Metadata:"));
}

#[test]
fn test_bead_excluded_by_label() {
    let exclude = vec!["security".to_string(), "escalation".to_string()];
    assert!(bead_excluded(&["security".to_string()], &exclude));
    assert!(bead_excluded(&["Escalation".to_string()], &exclude)); // case-insensitive
    assert!(bead_excluded(
        &["pitch".to_string(), "security".to_string()],
        &exclude
    ));
    assert!(!bead_excluded(&["pitch".to_string()], &exclude));
    assert!(!bead_excluded(&["security".to_string()], &[])); // empty = no exclusion
}

#[test]
fn test_content_hash_stable_and_sensitive() {
    let a = content_hash("hello");
    assert_eq!(a, content_hash("hello"));
    assert_ne!(a, content_hash("hello!"));
    assert_eq!(a.len(), 64);
}

#[test]
fn test_build_bead_content_includes_labels() {
    let issue = sample_issue();
    let labels = vec!["pitch".to_string(), "b:search".to_string()];
    let content = build_bead_content(&issue, &[], &labels);
    assert!(content.contains("Fix the widget"));
    assert!(content.contains("The widget is broken."));
    assert!(content.contains("Notes:\ninvestigated already"));
    assert!(content.contains("Labels: pitch, b:search"));
    assert!(content.contains("Status: open | Priority: P1 | Assignee: strider"));
}

#[test]
fn test_build_bead_content_no_labels_no_comments() {
    let issue = sample_issue();
    let content = build_bead_content(&issue, &[], &[]);
    assert!(!content.contains("Labels:"));
    assert!(!content.contains("Comments:"));
    assert!(content.contains("Assignee: strider"));
}

#[test]
fn test_build_bead_content_includes_comments() {
    let issue = sample_issue();
    let c = CommentRow {
        issue_id: "aegis-0a9".to_string(),
        author: "ian".to_string(),
        text: "looks good".to_string(),
    };
    let content = build_bead_content(&issue, &[&c], &[]);
    assert!(content.contains("Comments:"));
    assert!(content.contains("--- ian ---\nlooks good"));
}

#[test]
fn test_disabled_config_returns_empty() {
    let config = BeadsConfig::default();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let chunks = rt.block_on(fetch_beads(&config)).unwrap();
    assert!(chunks.is_empty());
}

#[test]
fn test_empty_databases_returns_empty() {
    let config = BeadsConfig {
        enabled: true,
        databases: vec![],
        ..Default::default()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let chunks = rt.block_on(fetch_beads(&config)).unwrap();
    assert!(chunks.is_empty());
}

// ---------------------------------------------------------------------------
// Single-bead path (GH#52 Phase 4)
// ---------------------------------------------------------------------------

fn open_config() -> BeadsConfig {
    BeadsConfig {
        enabled: true,
        databases: vec!["beads_aegis".to_string()],
        max_age_days: 90,
        ..Default::default()
    }
}

/// THE INVARIANT. Narrowing to one bead must add a predicate and remove none.
///
/// If `index-bead` relaxed the visibility rules, asking for a bead by name
/// would index rows `--include-beads` deliberately filters out — closed beads,
/// aged-out beads, deleted beads — and the two paths would disagree about what
/// the corpus contains, with the batch sweep silently deleting whatever the
/// single-bead path admitted on its next run.
#[test]
fn single_bead_narrows_without_relaxing() {
    let config = open_config();
    let batch = issues_where_clause(&config, None);
    let single = issues_where_clause(&config, Some("aegis-0a9"));

    // Every batch condition survives verbatim.
    assert!(
        single.starts_with(batch.as_str()),
        "batch={batch}\nsingle={single}"
    );
    // And exactly one condition is added.
    assert_eq!(
        single.matches(" AND ").count(),
        batch.matches(" AND ").count() + 1
    );
    assert!(single.ends_with("id = 'aegis-0a9'"));
}

#[test]
fn single_bead_clause_honours_include_closed() {
    let mut config = open_config();
    config.include_closed = true;
    let single = issues_where_clause(&config, Some("x-1"));
    assert!(single.contains("status != 'deleted'"));
    // Deleted rows stay excluded even when closed ones are wanted.
    assert!(!single.contains("status NOT IN ('closed', 'deleted') AND"));
}

#[test]
fn single_bead_clause_escapes_quotes() {
    let config = open_config();
    let single = issues_where_clause(&config, Some("o'brien-1"));
    assert!(single.ends_with("id = 'o''brien-1'"), "{single}");
}

/// The chunk id is derived from the chunk key, so `index_hashed_item` can
/// address the row the batch path wrote without re-querying it.
#[test]
fn chunk_id_is_the_hash_of_the_chunk_key() {
    let key = bead_file_path("aegis", "aegis-0a9");
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    assert_eq!(hex::encode(hasher.finalize()).len(), 64);
    assert_eq!(key, "beads:aegis:aegis-0a9");
}

#[test]
fn fetch_bead_is_a_no_op_when_beads_are_unconfigured() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Disabled.
    let chunks = rt
        .block_on(fetch_bead(&BeadsConfig::default(), "bo-1", None))
        .unwrap();
    assert!(chunks.is_empty());
    // Enabled but with no databases: no connection is attempted, so this must
    // return empty rather than erroring on a refused MySQL connect.
    let config = BeadsConfig {
        enabled: true,
        databases: vec![],
        ..Default::default()
    };
    let chunks = rt.block_on(fetch_bead(&config, "bo-1", None)).unwrap();
    assert!(chunks.is_empty());
}

/// A rig filter that matches nothing must skip every database rather than
/// falling through to "all of them".
#[test]
fn fetch_bead_with_an_unmatched_rig_touches_nothing() {
    let config = BeadsConfig {
        enabled: true,
        // Deliberately unroutable: if the filter leaked, this would try to
        // connect and the test would fail with a connection error instead.
        host: "192.0.2.1".to_string(),
        databases: vec!["beads_aegis".to_string()],
        ..Default::default()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let chunks = rt
        .block_on(fetch_bead(&config, "bo-1", Some("gastown")))
        .unwrap();
    assert!(chunks.is_empty());
}
