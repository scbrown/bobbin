//! Tests for the duplicate-bead advisory's pure parts (bobbin-bbe).
//!
//! The search itself needs an index and an embedding model; what is tested
//! here is everything that decides what a verdict *means* — status parsing,
//! bead-id extraction, and the corpus watermark. Those are the parts that
//! would silently mislead rather than loudly fail.

use super::*;

#[test]
fn test_status_is_parsed_from_the_line_the_indexer_writes() {
    let content =
        "Fix the thing\n\nSome description\n\nStatus: open | Priority: P2 | Assignee: unassigned";
    assert_eq!(Some("open".to_string()), parse_status(content));
}

#[test]
fn test_status_is_none_when_absent_rather_than_assumed_open() {
    // The load-bearing case. Defaulting an unknown status to "open" would
    // silently widen the corpus a caller asked to narrow, and the widening
    // would be invisible in the output.
    assert_eq!(None, parse_status("Fix the thing\n\nNo metadata line here"));
    assert_eq!(None, parse_status("Status: "));
}

#[test]
fn test_status_stops_at_the_field_separator() {
    let content = "t\n\nStatus: closed | Priority: P1 | Assignee: ian";
    assert_eq!(Some("closed".to_string()), parse_status(content));
}

#[test]
fn test_bead_id_is_extracted_from_the_indexed_path() {
    assert_eq!(
        Some("bobbin-aoz".to_string()),
        bead_id_from_path("beads:bobbin:bobbin-aoz")
    );
}

#[test]
fn test_a_bead_id_containing_colons_survives() {
    // splitn(3) keeps the remainder intact; a two-way split would truncate.
    assert_eq!(
        Some("ns:sub-1".to_string()),
        bead_id_from_path("beads:rig:ns:sub-1")
    );
}

#[test]
fn test_non_bead_paths_are_rejected() {
    assert_eq!(None, bead_id_from_path("src/cli/bead/mod.rs"));
    assert_eq!(None, bead_id_from_path("beads:rig:"));
    assert_eq!(None, bead_id_from_path("beads:rig"));
}

#[test]
fn test_the_watermark_is_order_independent() {
    // Two runs that searched the same corpus must report the same watermark,
    // or it cannot be used to explain a changed verdict.
    let mut a = vec!["b".to_string(), "a".to_string(), "c".to_string()];
    let mut b = vec!["c".to_string(), "b".to_string(), "a".to_string()];
    assert_eq!(watermark(&mut a), watermark(&mut b));
}

#[test]
fn test_the_watermark_changes_when_the_corpus_does() {
    // The guard: an order-independent digest that ignored content would pass
    // the test above and be useless.
    let mut a = vec!["a".to_string(), "b".to_string()];
    let mut b = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert_ne!(watermark(&mut a), watermark(&mut b));
}

#[test]
fn test_an_empty_corpus_still_produces_a_stable_watermark() {
    let mut empty: Vec<String> = vec![];
    let w = watermark(&mut empty);
    assert!(w.starts_with("sha256:"));
    assert_eq!(w, watermark(&mut Vec::<String>::new()));
}

#[test]
fn test_coverage_states_are_distinct() {
    // Empty and NoMatch render differently on purpose: "nothing was compared"
    // and "nothing matched" are different facts, and collapsing them is how a
    // dedup check quietly stops checking.
    assert_ne!(Coverage::Empty, Coverage::NoMatch);
    assert_ne!(Coverage::NoMatch, Coverage::Match);
    assert_eq!(
        "\"empty\"",
        serde_json::to_string(&Coverage::Empty).unwrap()
    );
    assert_eq!(
        "\"no_match\"",
        serde_json::to_string(&Coverage::NoMatch).unwrap()
    );
}
