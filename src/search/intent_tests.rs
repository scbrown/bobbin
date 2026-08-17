//! Tests for `super::intent`.
//!
//! Split out of `intent.rs` because adding the bobbin-lpp coverage pushed that
//! file from 428 to 530 lines, over the 500-line error limit — an eleventh
//! failure on a gate bobbin-aoz already calls out as permanently red. Files
//! matching `*tests.rs` are exempt by design (scripts/check-file-size.sh), so
//! this is the sanctioned home for them rather than an allowlist entry.

use super::*;

#[test]
fn test_classify_bugfix() {
    assert_eq!(classify_intent("fix the auth bug in login.rs"), QueryIntent::BugFix);
    assert_eq!(classify_intent("error[E0308]: mismatched types"), QueryIntent::BugFix);
    assert_eq!(classify_intent("the server is broken and crashes on startup"), QueryIntent::BugFix);
    assert_eq!(classify_intent("debug the failing test"), QueryIntent::BugFix);
}

#[test]
fn test_classify_architecture() {
    assert_eq!(classify_intent("how does the reactor pattern work in this codebase?"), QueryIntent::Architecture);
    assert_eq!(classify_intent("explain the architecture of the search module"), QueryIntent::Architecture);
    assert_eq!(classify_intent("what is the design pattern used here?"), QueryIntent::Architecture);
}

#[test]
fn test_classify_implementation() {
    assert_eq!(classify_intent("add a new endpoint for user profiles"), QueryIntent::Implementation);
    assert_eq!(classify_intent("implement rate limiting for the API"), QueryIntent::Implementation);
    assert_eq!(classify_intent("create a new config parser"), QueryIntent::Implementation);
    assert_eq!(classify_intent("add rate limiting to the API endpoint"), QueryIntent::Implementation);
    assert_eq!(classify_intent("add logging to the auth module"), QueryIntent::Implementation);
}

#[test]
fn test_classify_configuration() {
    assert_eq!(classify_intent("how to configure nginx for this service"), QueryIntent::Configuration);
    assert_eq!(classify_intent("set up the docker environment"), QueryIntent::Configuration);
    assert_eq!(classify_intent("update the deploy yaml config"), QueryIntent::Configuration);
    assert_eq!(classify_intent("deploy the service to production"), QueryIntent::Configuration);
    assert_eq!(classify_intent("deploy this to node-4"), QueryIntent::Configuration);
}

#[test]
fn test_classify_navigation() {
    assert_eq!(classify_intent("where is the main entry point defined?"), QueryIntent::Navigation);
    assert_eq!(classify_intent("which file handles authentication?"), QueryIntent::Navigation);
    assert_eq!(classify_intent("find the database connection code"), QueryIntent::Navigation);
}

#[test]
fn test_classify_operational() {
    assert_eq!(classify_intent("git push"), QueryIntent::Operational);
    assert_eq!(classify_intent("cargo test"), QueryIntent::Operational);
    assert_eq!(classify_intent("run the tests"), QueryIntent::Operational);
    assert_eq!(classify_intent("commit this and push"), QueryIntent::Operational);
    assert_eq!(classify_intent("bd close aegis-abc"), QueryIntent::Operational);
    assert_eq!(classify_intent("check if tests pass"), QueryIntent::Operational);
}

#[test]
fn test_classify_operational_workflow() {
    // Agent workflow queries should be Operational, not General
    assert_eq!(classify_intent("what's next on my hook"), QueryIntent::Operational);
    assert_eq!(classify_intent("check my hook and mail"), QueryIntent::Operational);
    assert_eq!(classify_intent("ready beads to pick up"), QueryIntent::Operational);
    assert_eq!(classify_intent("check inbox for work"), QueryIntent::Operational);
}

#[test]
fn test_classify_operational_bead_queries() {
    // Bead assignment/status queries should be Operational
    assert_eq!(classify_intent("what beads are assigned to me"), QueryIntent::Operational);
    assert_eq!(classify_intent("show my open beads"), QueryIntent::Operational);
    assert_eq!(classify_intent("close this bead and pick next"), QueryIntent::Operational);
}

#[test]
fn test_classify_operational_status_check() {
    // "check" + "status" stems should be Operational
    assert_eq!(classify_intent("check the patrol status and queue"), QueryIntent::Operational);
    assert_eq!(classify_intent("close your beads when done"), QueryIntent::Operational);
    assert_eq!(classify_intent("check on the deployment"), QueryIntent::Operational);
}

#[test]
fn test_classify_operational_monitoring() {
    // Infrastructure monitoring queries should be Operational, not Architecture
    assert_eq!(classify_intent("what is the disk usage on seeker2"), QueryIntent::Operational);
    assert_eq!(classify_intent("how much disk space is left"), QueryIntent::Operational);
    assert_eq!(classify_intent("check the memory usage on node-4"), QueryIntent::Operational);
    assert_eq!(classify_intent("check the backup status"), QueryIntent::Operational);
    assert_eq!(classify_intent("restart the service"), QueryIntent::Operational);
    assert_eq!(classify_intent("check cert expiry on traefik"), QueryIntent::Operational);
    assert_eq!(classify_intent("disk usage on the server"), QueryIntent::Operational);
    assert_eq!(classify_intent("alert firing on prometheus"), QueryIntent::Operational);
}

#[test]
fn test_classify_operational_short_commands() {
    // Short bead management commands should be Operational
    assert_eq!(classify_intent("remove bo-qq5h"), QueryIntent::Operational);
    assert_eq!(classify_intent("hook c9y9wm and handoff"), QueryIntent::Operational);
    assert_eq!(classify_intent("delete this bead"), QueryIntent::Operational);
    assert_eq!(classify_intent("show aegis-abc"), QueryIntent::Operational);
    assert_eq!(classify_intent("sling gt-xyz aegis"), QueryIntent::Operational);
}

#[test]
fn test_classify_general() {
    assert_eq!(classify_intent("hello"), QueryIntent::General);
    assert_eq!(classify_intent("thanks"), QueryIntent::General);
}

#[test]
fn test_adjustments_operational_raises_gate() {
    let adj = intent_adjustments(QueryIntent::Operational);
    assert!(adj.gate_boost > 0.0); // Raises the gate threshold
    assert!(adj.doc_demotion_factor > 1.0); // Demotes docs
}

// --- bobbin-lpp: the factor must mean the same thing on both hook paths ---

#[test]
fn test_doc_demotion_factor_above_one_strengthens_demotion() {
    // doc_demotion is a MULTIPLIER, so stronger demotion means a LOWER value.
    // The pre-fix local path computed 0.5 * 1.5 = 0.75 here and weakened it.
    let result = apply_doc_demotion_factor(0.5, 1.5, 0.0);
    assert!(result < 0.5, "factor > 1.0 must lower the multiplier, got {result}");
    assert!((result - 0.25).abs() < f32::EPSILON, "got {result}");
}

#[test]
fn test_doc_demotion_factor_below_one_weakens_demotion() {
    // Architecture's 0.3: docs should become MORE visible, i.e. higher multiplier.
    let result = apply_doc_demotion_factor(0.5, 0.3, 0.0);
    assert!(result > 0.5, "factor < 1.0 must raise the multiplier, got {result}");
    assert!((result - 0.85).abs() < f32::EPSILON, "got {result}");
}

#[test]
fn test_doc_demotion_factor_of_one_is_identity() {
    for base in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let result = apply_doc_demotion_factor(base, 1.0, 0.0);
        assert!((result - base).abs() < f32::EPSILON, "base {base} -> {result}");
    }
}

#[test]
fn test_every_shipped_intent_moves_demotion_the_direction_its_comment_claims() {
    // The table in intent_adjustments carries comments like "Demote docs more".
    // This checks the arithmetic actually delivers that, for every intent —
    // which is the check that would have caught bobbin-lpp at the source.
    let base = 0.5;
    for intent in [
        QueryIntent::BugFix,
        QueryIntent::Architecture,
        QueryIntent::Implementation,
        QueryIntent::Configuration,
        QueryIntent::Navigation,
        QueryIntent::Operational,
        QueryIntent::General,
    ] {
        let factor = intent_adjustments(intent).doc_demotion_factor;
        let result = apply_doc_demotion_factor(base, factor, 0.0);
        if factor > 1.0 {
            assert!(result < base, "{intent:?}: factor {factor} should demote more");
        } else if factor < 1.0 {
            assert!(result > base, "{intent:?}: factor {factor} should demote less");
        } else {
            assert!((result - base).abs() < f32::EPSILON, "{intent:?}");
        }
    }
}

#[test]
fn test_doc_demotion_floor_is_respected() {
    // Operational's 2.0 against a low base drives the effect past 1.0; the
    // local path must not hand back a multiplier of exactly zero.
    assert_eq!(apply_doc_demotion_factor(0.0, 2.0, 0.01), 0.01);
    assert_eq!(apply_doc_demotion_factor(0.0, 2.0, 0.0), 0.0);
}

#[test]
fn test_result_stays_a_valid_multiplier_for_extreme_factors() {
    for factor in [0.0, 0.1, 1.0, 2.0, 10.0] {
        for base in [0.0, 0.5, 1.0] {
            let result = apply_doc_demotion_factor(base, factor, 0.01);
            assert!((0.01..=1.0).contains(&result), "base {base} factor {factor} -> {result}");
        }
    }
}

#[test]
fn test_adjustments_bugfix_prefers_code() {
    let adj = intent_adjustments(QueryIntent::BugFix);
    assert!(adj.doc_demotion_factor > 1.0); // More demotion = less docs
    assert!(adj.recency_weight_factor > 1.0); // Prefer recent
}

#[test]
fn test_adjustments_architecture_prefers_docs() {
    let adj = intent_adjustments(QueryIntent::Architecture);
    assert!(adj.doc_demotion_factor < 1.0); // Less demotion = more docs
}

#[test]
fn test_adjustments_general_has_slight_gate_boost() {
    let adj = intent_adjustments(QueryIntent::General);
    assert!((adj.doc_demotion_factor - 1.0).abs() < f32::EPSILON);
    assert!((adj.semantic_weight_factor - 1.0).abs() < f32::EPSILON);
    assert!((adj.recency_weight_factor - 1.0).abs() < f32::EPSILON);
    assert!(adj.gate_boost > 0.0, "General intent should have slight gate boost");
    assert!(adj.gate_boost <= 0.15, "General gate boost should be moderate");
}

#[test]
fn test_classify_navigation_expanded() {
    // New navigation phrases added in Wave 18b
    assert_eq!(classify_intent("look at src/main.rs"), QueryIntent::Navigation);
    assert_eq!(classify_intent("read the config file"), QueryIntent::Navigation);
    assert_eq!(classify_intent("search for the auth handler"), QueryIntent::Navigation);
    assert_eq!(classify_intent("grep for error handling"), QueryIntent::Navigation);
}

#[test]
fn test_classify_operational_review() {
    // Review/diff queries should be Operational
    assert_eq!(classify_intent("review the pr for auth changes"), QueryIntent::Operational);
    assert_eq!(classify_intent("what changed in the last commit"), QueryIntent::Operational);
    assert_eq!(classify_intent("show the diff"), QueryIntent::Operational);
    assert_eq!(classify_intent("git show HEAD"), QueryIntent::Operational);
}
