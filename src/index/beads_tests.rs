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
    assert!(bead_excluded(&["pitch".to_string(), "security".to_string()], &exclude));
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
