//! Tests for `index-bead`'s pure decision logic.
//!
//! The Dolt fetch and the embed/insert half need a live MySQL-protocol server
//! and an ONNX model, so what is asserted here is everything that can be wrong
//! WITHOUT them: the key set the command sweeps, the corpus it writes into,
//! and the outcome it reports when a bead appears in more than one rig.

use super::*;
use crate::config::BeadsConfig;
use crate::index::beads::{bead_file_path, bead_file_paths, rig_of};

fn cfg(dbs: &[&str]) -> BeadsConfig {
    BeadsConfig {
        enabled: true,
        databases: dbs.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// The two paths must write the same corpus. If this ever fails, a single-bead
/// reindex is writing rows the batch sweep cannot see and vice versa — each
/// silently accumulating a shadow copy of the other's beads.
#[test]
fn index_bead_uses_the_batch_repo_key() {
    assert_eq!(BEADS_HASH_REPO, "beads-issues");
    assert_eq!(BEADS_HASH_REPO, crate::cli::index::BEADS_HASH_REPO);
    assert_eq!(BEADS_SOURCE_LABEL, "beads");
}

#[test]
fn rig_names_drop_the_beads_prefix() {
    assert_eq!(rig_of("beads_aegis"), "aegis");
    assert_eq!(rig_of("beads_gastown"), "gastown");
    // A database not following the convention is its own rig name, not a panic.
    assert_eq!(rig_of("scratch"), "scratch");
    // Only the leading prefix is stripped, once.
    assert_eq!(rig_of("beads_beads_x"), "beads_x");
}

/// The key is the identity the incremental machinery hashes against, so its
/// exact spelling is a contract with the batch path, not an implementation
/// detail. `docs/plans/beads-integration.md` states it as `beads:{rig}:{id}`.
#[test]
fn bead_keys_match_the_documented_shape() {
    assert_eq!(
        bead_file_path("aegis", "aegis-0a9"),
        "beads:aegis:aegis-0a9"
    );
}

/// Without a rig filter the command must visit EVERY configured rig's key.
/// Visiting only the rigs that returned a chunk would make removal impossible:
/// a bead that just closed returns nothing, and the key that needs sweeping is
/// exactly the one with no chunk behind it.
#[test]
fn candidate_keys_cover_every_configured_rig() {
    let c = cfg(&["beads_aegis", "beads_gastown", "beads_bobbin"]);
    assert_eq!(
        bead_file_paths(&c, "bo-1", None),
        vec![
            "beads:aegis:bo-1",
            "beads:gastown:bo-1",
            "beads:bobbin:bo-1"
        ]
    );
}

#[test]
fn rig_filter_narrows_to_one_key() {
    let c = cfg(&["beads_aegis", "beads_gastown"]);
    assert_eq!(
        bead_file_paths(&c, "bo-1", Some("gastown")),
        vec!["beads:gastown:bo-1"]
    );
    // A rig that is not configured yields no keys — the caller rejects it up
    // front, so this must not silently fall back to "all rigs".
    assert!(bead_file_paths(&c, "bo-1", Some("nope")).is_empty());
}

#[test]
fn no_configured_databases_means_no_keys() {
    assert!(bead_file_paths(&cfg(&[]), "bo-1", None).is_empty());
}

/// A run that changed something must never report as if it did nothing. With
/// one id present in two rigs, one re-embedded and one untouched, the reported
/// status is `indexed`.
#[test]
fn the_most_significant_outcome_is_reported() {
    assert!(outcome_rank(ItemOutcome::Indexed) > outcome_rank(ItemOutcome::Removed));
    assert!(outcome_rank(ItemOutcome::Removed) > outcome_rank(ItemOutcome::Unchanged));
    assert!(outcome_rank(ItemOutcome::Unchanged) > outcome_rank(ItemOutcome::Absent));
}

#[test]
fn outcome_labels_are_the_documented_json_values() {
    assert_eq!(outcome_label(ItemOutcome::Indexed), "indexed");
    assert_eq!(outcome_label(ItemOutcome::Unchanged), "unchanged");
    assert_eq!(outcome_label(ItemOutcome::Removed), "removed");
    assert_eq!(outcome_label(ItemOutcome::Absent), "absent");
}
