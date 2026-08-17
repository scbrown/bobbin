//! Tests for `InjectionTurn` — ledger and injection identity on the
//! non-prompt injecting paths (bobbin-aa0).
//!
//! Own file for the same reason as `hook_session_id_tests.rs`: `hook.rs` is
//! the largest file in the tree and allowlisted out of the file-size gate.

use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// The gap as filed: these paths never consulted the ledger, so they could
/// re-deliver a chunk the prompt path had already sent. A turn must not claim
/// what a previous turn recorded.
#[test]
fn test_a_chunk_recorded_by_a_previous_turn_cannot_be_claimed() {
    let dir = tmp();
    let root = dir.path();

    // Simulate the prompt path having already delivered this chunk.
    let mut prior = SessionLedger::load(root, "sess-1");
    prior.record(&[chunk_key("src/a.rs", 10, 20)], "inj-prompt");

    let mut turn = InjectionTurn::open(root, "sess-1", true, "some error");
    assert!(
        !turn.claim("src/a.rs", 10, 20),
        "claimed a chunk the prompt path already injected"
    );
    assert!(
        turn.claim("src/b.rs", 1, 5),
        "a fresh chunk must be claimable"
    );
    assert_eq!(turn.len(), 1);
}

/// Within one turn, the same chunk must not be emitted twice — the failure
/// handler's direct pass can reach the same file through an error ref and
/// again through coupling.
#[test]
fn test_a_chunk_cannot_be_claimed_twice_in_one_turn() {
    let dir = tmp();
    let mut turn = InjectionTurn::open(dir.path(), "sess-1", true, "q");
    assert!(turn.claim("src/a.rs", 1, 9));
    assert!(!turn.claim("src/a.rs", 1, 9));
    assert_eq!(turn.len(), 1);
}

/// Within-turn dedup must hold even with reducing disabled. Filtering across
/// turns is configurable; emitting the same chunk twice in one response is
/// never wanted.
#[test]
fn test_within_turn_dedup_applies_even_when_reducing_is_disabled() {
    let dir = tmp();
    let mut turn = InjectionTurn::open(dir.path(), "sess-1", false, "q");
    assert!(turn.claim("src/a.rs", 1, 9));
    assert!(!turn.claim("src/a.rs", 1, 9));
}

/// With reducing disabled the turn must NOT filter against prior turns, but
/// must still carry an identifier — the record is what makes an injection
/// rateable, and that is independent of delta filtering.
#[test]
fn test_reducing_disabled_still_yields_an_identifier_and_no_cross_turn_filtering() {
    let dir = tmp();
    let root = dir.path();
    let mut prior = SessionLedger::load(root, "sess-1");
    prior.record(&[chunk_key("src/a.rs", 10, 20)], "inj-prompt");

    let mut turn = InjectionTurn::open(root, "sess-1", false, "q");
    assert!(
        turn.claim("src/a.rs", 10, 20),
        "reducing is off; prior turns must not filter"
    );
    assert!(turn.injection_id().starts_with("inj-"));
}

/// No session id: filtering degrades to in-memory, and the turn still has an
/// identifier. The caller must not have to branch on this.
#[test]
fn test_no_session_id_degrades_to_in_memory_but_keeps_an_identifier() {
    let dir = tmp();
    let mut turn = InjectionTurn::open(dir.path(), "", true, "q");
    assert!(turn.injection_id().starts_with("inj-"));
    assert!(turn.claim("src/a.rs", 1, 2));
    assert!(!turn.claim("src/a.rs", 1, 2));
    turn.commit(); // must not panic with no ledger path
}

/// `commit` must make this turn's chunks visible to the next one — that is
/// the whole point of recording, and the bead's "bypass the session ledger"
/// half.
#[test]
fn test_commit_makes_chunks_visible_to_the_next_turn() {
    let dir = tmp();
    let root = dir.path();

    let mut first = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(first.claim("src/a.rs", 1, 9));
    first.commit();

    let mut second = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(
        !second.claim("src/a.rs", 1, 9),
        "the next turn re-injected what this one recorded"
    );
}

/// The mirror of the test above, and the reason `commit` is called only after
/// output: an uncommitted turn must leave no trace. A handler that bailed
/// before writing its response must not suppress those chunks next turn.
#[test]
fn test_an_uncommitted_turn_leaves_the_ledger_untouched() {
    let dir = tmp();
    let root = dir.path();

    let mut abandoned = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(abandoned.claim("src/a.rs", 1, 9));
    drop(abandoned); // no commit — the handler returned early

    let mut next = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(
        next.claim("src/a.rs", 1, 9),
        "chunks were suppressed by a turn that never emitted them"
    );
}

/// Committing an empty turn must not advance the ledger or write a record.
#[test]
fn test_committing_an_empty_turn_is_a_no_op() {
    let dir = tmp();
    let root = dir.path();
    let mut turn = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(turn.is_empty());
    turn.commit();

    let ledger = SessionLedger::load(root, "sess-1");
    assert_eq!(ledger.turn, 0, "an empty commit advanced the turn counter");
}

/// File-level claims use the `path:0:0` marker convention that complementary
/// expansion already writes, so the two collide as they should.
#[test]
fn test_file_claims_collide_with_the_complementary_expansion_marker() {
    let dir = tmp();
    let root = dir.path();

    let mut prior = SessionLedger::load(root, "sess-1");
    prior.record(&[chunk_key("src/a.rs", 0, 0)], "inj-complementary");

    let mut turn = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(
        !turn.claim_file("src/a.rs"),
        "re-suggested a file complementary expansion already suggested"
    );
}

/// The deliberate asymmetry: a file marker must NOT suppress a later
/// chunk-level injection of the same file. Naming a file and showing its
/// contents are different deliveries.
#[test]
fn test_a_file_marker_does_not_suppress_a_chunk_from_that_file() {
    let dir = tmp();
    let root = dir.path();

    let mut prior = SessionLedger::load(root, "sess-1");
    prior.record(&[chunk_key("src/a.rs", 0, 0)], "inj-post-tool-use");

    let mut turn = InjectionTurn::open(root, "sess-1", true, "q");
    assert!(
        turn.claim("src/a.rs", 40, 60),
        "a file suggestion wrongly suppressed injecting that file's contents"
    );
}

/// `claimed_files` feeds the injection record. It must deduplicate, preserve
/// claim order, and parse paths back out of the composite key correctly —
/// including paths that themselves contain colons.
#[test]
fn test_claimed_files_dedupes_preserves_order_and_handles_colons_in_paths() {
    let dir = tmp();
    let mut turn = InjectionTurn::open(dir.path(), "sess-1", true, "q");
    assert!(turn.claim("src/b.rs", 1, 5));
    assert!(turn.claim("src/a.rs", 1, 5));
    assert!(turn.claim("src/b.rs", 20, 25)); // same file, second chunk
    assert!(turn.claim("C:/win/path.rs", 1, 2));

    assert_eq!(
        turn.claimed_files(),
        vec![
            "src/b.rs".to_string(),
            "src/a.rs".to_string(),
            "C:/win/path.rs".to_string(),
        ],
    );
}

/// Two turns must not share an identifier, or feedback on one would be
/// attributed to the other.
#[test]
fn test_turns_get_distinct_identifiers() {
    let dir = tmp();
    let a = InjectionTurn::open(dir.path(), "sess-1", true, "query one");
    let b = InjectionTurn::open(dir.path(), "sess-1", true, "query two");
    assert_ne!(a.injection_id(), b.injection_id());
}
