//! Tests for the governed-tripwire surface.
//!
//! The fixtures are shaped like quipu's real catalog
//! (`/home/user/quipu/shapes/policies/tripwire.ttl` — `tripwire-auth-boundary`
//! and `tripwire-generated-throttle`) rather than like a minimal happy path, so
//! a change in the upstream vocabulary shows up here as a failing assertion
//! instead of as an empty section in someone's prompt.
//!
//! What is NOT proven here, stated plainly: there is no quipu server in this
//! environment, so no test exercises the HTTP transport end to end. The decode,
//! the matching, the cache's staleness accounting and the rendering are proven;
//! "bobbin can actually reach a live quipu and this query returns these rows"
//! is not, and the query text is held to yupana's by inspection.

use super::*;
use std::path::Path;

fn binding(v: &str) -> serde_json::Value {
    serde_json::json!({ "type": "literal", "value": v })
}

fn row(pairs: &[(&str, &str)]) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), binding(v));
    }
    serde_json::Value::Object(m)
}

fn results(rows: &[serde_json::Value]) -> String {
    serde_json::json!({ "results": { "bindings": rows } }).to_string()
}

/// The shipped `tripwire-auth-boundary` policy, as quipu serves it.
fn auth_rows() -> Vec<serde_json::Value> {
    vec![row(&[
        (
            "policy",
            "http://aegis.gastown.local/ontology/policy_tripwire_auth_boundary",
        ),
        ("name", "tripwire-auth-boundary"),
        ("appliesTo", "src/auth/**"),
        ("effect", "deny"),
        ("claim", "no agent edit targets a path matching src/auth/**"),
        ("constraintClass", "hard"),
        ("verificationPoint", "PAG"),
    ])]
}

// ---------------------------------------------------------------------------
// decode
// ---------------------------------------------------------------------------

#[test]
fn a_multi_glob_policy_accumulates_its_globs_rather_than_keeping_the_first() {
    let json = results(&[
        row(&[
            ("policy", "http://x/p1"),
            ("name", "auth-boundary"),
            ("appliesTo", "src/auth/**"),
            ("effect", "deny"),
        ]),
        row(&[
            ("policy", "http://x/p1"),
            ("name", "auth-boundary"),
            ("appliesTo", "src/session/**"),
            ("effect", "deny"),
        ]),
    ]);
    let wires = decode_tripwires(&json).unwrap();
    assert_eq!(wires.len(), 1, "one policy, not one per row");
    assert_eq!(
        wires[0].paths,
        vec!["src/auth/**".to_string(), "src/session/**".to_string()],
        "a boundary that silently shrank is the failure this guards"
    );
    assert!(wires[0].conflicts.is_empty());
}

#[test]
fn a_rule_policy_is_left_to_its_own_plane_and_is_not_a_tripwire() {
    // A selector (or predicate) makes it a rule policy by quipu's definition.
    let json = results(&[row(&[
        ("policy", "http://x/rule1"),
        ("appliesTo", "src/**"),
        ("effect", "deny"),
        ("selector", "http://x/sel1"),
    ])]);
    assert!(decode_tripwires(&json).unwrap().is_empty());

    let json = results(&[row(&[
        ("policy", "http://x/rule2"),
        ("appliesTo", "src/**"),
        ("effect", "deny"),
        ("predicate", "http://x/pred1"),
    ])]);
    assert!(decode_tripwires(&json).unwrap().is_empty());
}

#[test]
fn an_effect_bobbin_cannot_enforce_is_surfaced_not_dropped() {
    // Yupana refuses these because it must ENFORCE. Bobbin only has to NAME
    // them, and a dropped wire here is a boundary nobody is warned about.
    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("name", "needs-approval"),
        ("appliesTo", "src/**"),
        ("effect", "require-approval"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    assert_eq!(wires.len(), 1);
    assert_eq!(
        wires[0].effect,
        TripEffect::Other("require-approval".to_string())
    );
    assert_eq!(wires[0].effect.as_str(), "require-approval");
    assert!(wires[0].is_well_formed());
}

#[test]
fn a_policy_with_no_effect_survives_and_reports_the_defect() {
    let json = results(&[row(&[("policy", "http://x/p1"), ("appliesTo", "src/**")])]);
    let wires = decode_tripwires(&json).unwrap();
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].effect, TripEffect::Undeclared);
    assert!(!wires[0].is_well_formed());
    assert!(wires[0].defect().unwrap().contains("aegis:effect"));
}

#[test]
fn an_unlabelled_policy_is_named_by_its_iri_tail() {
    let json = results(&[row(&[
        (
            "policy",
            "http://aegis.gastown.local/ontology/policy_tripwire_x",
        ),
        ("appliesTo", "src/**"),
        ("effect", "warn"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    assert_eq!(wires[0].name, "policy_tripwire_x");
}

#[test]
fn conflicting_rows_mark_the_wire_instead_of_picking_one_or_dropping_the_batch() {
    let json = results(&[
        row(&[
            ("policy", "http://x/p1"),
            ("appliesTo", "a/**"),
            ("effect", "deny"),
        ]),
        row(&[
            ("policy", "http://x/p1"),
            ("appliesTo", "b/**"),
            ("effect", "warn"),
        ]),
        // A second, well-formed policy must survive the first one's conflict.
        row(&[
            ("policy", "http://x/p2"),
            ("appliesTo", "c/**"),
            ("effect", "warn"),
        ]),
    ]);
    let wires = decode_tripwires(&json).unwrap();
    assert_eq!(wires.len(), 2, "one bad policy must not void the batch");
    assert_eq!(wires[0].conflicts, vec!["aegis:effect".to_string()]);
    assert!(!wires[0].is_well_formed());
    // The BOUNDARY still accumulated — that part was never in conflict.
    assert_eq!(wires[0].paths, vec!["a/**".to_string(), "b/**".to_string()]);
    assert!(wires[1].is_well_formed());
}

#[test]
fn a_throttle_with_no_backoff_formula_is_reported_as_malformed() {
    // Quipu's placement gate refuses this, so seeing one means something
    // upstream is wrong and hiding it would hide the bug.
    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("name", "hot-tree"),
        ("appliesTo", "src/generated/**"),
        ("effect", "throttle"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    assert!(!wires[0].is_well_formed());
    assert!(wires[0].defect().unwrap().contains("backoffFormula"));

    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("name", "hot-tree"),
        ("appliesTo", "src/generated/**"),
        ("effect", "throttle"),
        ("backoffFormula", "exp(min(overage / 1.0, 8.0))"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    assert!(wires[0].is_well_formed());
}

#[test]
fn a_non_sparql_payload_is_an_error_not_an_empty_projection() {
    // "quipu answered with garbage" must not read as "there are no wires".
    assert!(decode_tripwires("not json at all").is_err());
    assert!(decode_tripwires(r#"{"ok": true}"#).is_err());
    // An empty result set IS a valid empty projection.
    assert!(decode_tripwires(&results(&[])).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// path matching
// ---------------------------------------------------------------------------

#[test]
fn a_server_prefixed_path_is_split_into_repo_and_repo_relative_path() {
    let (repo, rel) = render::split_repo(
        "/data/repos/aegis/src/auth/login.rs",
        Some("/data/repos"),
        Path::new("/home/user/bobbin"),
    );
    assert_eq!(repo.as_deref(), Some("aegis"));
    assert_eq!(rel, "src/auth/login.rs");
}

#[test]
fn an_absolute_path_under_the_repo_root_is_made_relative_to_it() {
    let (repo, rel) = render::split_repo(
        "/home/user/bobbin/src/auth/login.rs",
        None,
        Path::new("/home/user/bobbin"),
    );
    assert_eq!(repo.as_deref(), Some("bobbin"));
    assert_eq!(rel, "src/auth/login.rs");
}

#[test]
fn an_unrecognised_absolute_path_is_not_forced_to_fit_a_glob() {
    // No speculative suffix-stripping: manufacturing a repo root to make
    // `src/auth/**` match would invent a boundary that does not exist.
    let (repo, rel) = render::split_repo(
        "/somewhere/else/src/auth/login.rs",
        Some("/data/repos"),
        Path::new("/home/user/bobbin"),
    );
    assert_eq!(repo, None);
    assert_eq!(rel, "/somewhere/else/src/auth/login.rs");
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    assert!(matching(
        &wires,
        &["/somewhere/else/src/auth/login.rs".to_string()],
        Some("/data/repos"),
        Path::new("/home/user/bobbin"),
    )
    .is_empty());
}

#[test]
fn a_wire_matches_only_the_files_inside_its_boundary() {
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    let paths = vec![
        "src/auth/login.rs".to_string(),
        "src/search/query.rs".to_string(),
        "src/auth/session/mod.rs".to_string(),
    ];
    let m = matching(&wires, &paths, None, Path::new("/repo"));
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].files.len(), 2);
    assert!(m[0].files.iter().all(|(_, r)| r.starts_with("src/auth/")));
}

#[test]
fn a_malformed_glob_never_matches_and_never_panics() {
    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("appliesTo", "src/["),
        ("effect", "deny"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    assert!(matching(
        &wires,
        &["src/[".to_string(), "src/a.rs".to_string()],
        None,
        Path::new("/repo"),
    )
    .is_empty());
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

fn live() -> Provenance {
    Provenance::Live {
        endpoint: "http://quipu.test".to_string(),
    }
}

#[test]
fn nothing_matched_and_the_projection_is_current_renders_nothing() {
    // Silence is correct here and only here: the graph was read and it said no.
    assert!(section(&[], &live(), "standard").is_none());
}

#[test]
fn nothing_matched_but_the_refresh_failed_still_says_so() {
    // "I could not look" must not be indistinguishable from "there is nothing".
    let stale = Provenance::Cached {
        endpoint: "http://quipu.test".to_string(),
        age_secs: 4000,
        refresh_error: Some("connection refused".to_string()),
    };
    let out = section(&[], &stale, "standard").expect("must speak");
    assert!(out.contains("could not be refreshed"));
    assert!(out.contains("connection refused"));
    assert!(out.contains("last-known"));
}

#[test]
fn a_cached_projection_inside_the_ttl_does_not_cry_wolf() {
    let fresh_cache = Provenance::Cached {
        endpoint: "http://quipu.test".to_string(),
        age_secs: 30,
        refresh_error: None,
    };
    assert!(section(&[], &fresh_cache, "standard").is_none());
    assert!(fresh_cache.note().contains("within refresh interval"));
    assert!(!fresh_cache.note().contains("FAILED"));
}

#[test]
fn a_rendered_wire_carries_its_claim_boundary_placement_and_source() {
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    let m = matching(
        &wires,
        &["src/auth/login.rs".to_string()],
        None,
        Path::new("/repo"),
    );
    let out = section(&m, &live(), "standard").unwrap();
    assert!(out.contains("tripwire-auth-boundary"));
    assert!(out.contains("effect: deny"));
    assert!(out.contains("no agent edit targets a path matching src/auth/**"));
    assert!(out.contains("boundary: src/auth/**"));
    assert!(out.contains("in context: src/auth/login.rs"));
    assert!(out.contains("hard"));
    assert!(out.contains("pre-action gate"));
    assert!(out.contains("http://quipu.test"));
}

#[test]
fn the_section_never_claims_bobbin_enforces_anything() {
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    let m = matching(
        &wires,
        &["src/auth/login.rs".to_string()],
        None,
        Path::new("/repo"),
    );
    let out = section(&m, &live(), "standard").unwrap();
    assert!(
        out.contains("Bobbin does not enforce these"),
        "a surfacing tool that reads as a gate is the inverted armed-inert defect"
    );
}

#[test]
fn a_paa_wire_says_it_does_not_stop_this_edit() {
    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("name", "tripwire-generated-throttle"),
        ("appliesTo", "src/generated/**"),
        ("effect", "throttle"),
        ("constraintClass", "soft"),
        ("verificationPoint", "PAA"),
        ("backoffFormula", "exp(min(overage / 1.0, 8.0))"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    let m = matching(
        &wires,
        &["src/generated/api.rs".to_string()],
        None,
        Path::new("/repo"),
    );
    let out = section(&m, &live(), "standard").unwrap();
    assert!(out.contains("soft"));
    assert!(out.contains("does not stop this edit"));
    assert!(!out.contains("MALFORMED"));
}

#[test]
fn a_malformed_wire_is_rendered_with_its_defect_named() {
    let json = results(&[row(&[
        ("policy", "http://x/p1"),
        ("name", "bare-throttle"),
        ("appliesTo", "src/**"),
        ("effect", "throttle"),
    ])]);
    let wires = decode_tripwires(&json).unwrap();
    let m = matching(&wires, &["src/a.rs".to_string()], None, Path::new("/repo"));
    let out = section(&m, &live(), "standard").unwrap();
    assert!(out.contains("MALFORMED"));
    assert!(out.contains("backoffFormula"));
}

#[test]
fn the_multi_repo_ambiguity_is_stated_only_when_it_actually_arises() {
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    let one_repo = matching(
        &wires,
        &["/data/repos/aegis/src/auth/a.rs".to_string()],
        Some("/data/repos"),
        Path::new("/repo"),
    );
    let out = section(&one_repo, &live(), "standard").unwrap();
    assert!(!out.contains("carry no repo scoping"));

    let two_repos = matching(
        &wires,
        &[
            "/data/repos/aegis/src/auth/a.rs".to_string(),
            "/data/repos/bobbin/src/auth/b.rs".to_string(),
        ],
        Some("/data/repos"),
        Path::new("/repo"),
    );
    let out = section(&two_repos, &live(), "standard").unwrap();
    assert!(out.contains("carry no repo scoping"));
    assert!(out.contains("aegis:src/auth/a.rs"));
    assert!(out.contains("bobbin:src/auth/b.rs"));
}

#[test]
fn xml_format_mode_wraps_the_section_in_a_tag() {
    let wires = decode_tripwires(&results(&auth_rows())).unwrap();
    let m = matching(
        &wires,
        &["src/auth/a.rs".to_string()],
        None,
        Path::new("/r"),
    );
    let out = section(&m, &live(), "xml").unwrap();
    assert!(out.starts_with("<bobbin-governance>"));
    assert!(out.trim_end().ends_with("</bobbin-governance>"));
}

#[test]
fn a_long_projection_is_truncated_and_says_how_much_it_held_back() {
    let rows: Vec<serde_json::Value> = (0..10)
        .map(|i| {
            row(&[
                ("policy", &format!("http://x/p{i}")),
                ("name", &format!("wire-{i}")),
                ("appliesTo", "src/**"),
                ("effect", "warn"),
            ])
        })
        .collect();
    let wires = decode_tripwires(&results(&rows)).unwrap();
    let m = matching(&wires, &["src/a.rs".to_string()], None, Path::new("/r"));
    assert_eq!(m.len(), 10);
    let out = section(&m, &live(), "standard").unwrap();
    assert!(out.contains("and 4 further wire(s)"));
    assert!(out.contains("wire-0"));
    assert!(!out.contains("wire-9"));
}

// ---------------------------------------------------------------------------
// configuration and cost
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_ungoverned_deployment_returns_the_text_untouched_and_makes_no_call() {
    // No endpoint => no HTTP, no section, no cost on the hook's hot path.
    // (Env override cleared so a developer's shell cannot make this pass or
    // fail by accident.)
    let mut config = crate::config::Config::default();
    config.quipu_endpoint = None;
    std::env::remove_var("BOBBIN_QUIPU_REMOTE");
    let dir = tempfile::tempdir().unwrap();
    let out = with_boundaries(
        "CONTEXT".to_string(),
        &config,
        vec!["src/auth/a.rs".to_string()],
        dir.path(),
        "standard",
    )
    .await;
    assert_eq!(out, "CONTEXT");
    assert!(!cache::cache_path(dir.path()).exists());
}

#[test]
fn the_cache_lives_under_the_repo_and_not_in_a_shared_temp_dir() {
    let p = cache::cache_path(Path::new("/repo"));
    assert!(p.ends_with(".bobbin/tripwire-cache.json"));
}

#[test]
fn ages_are_rendered_the_way_a_sentence_wants_them() {
    assert_eq!(human_age(45), "45s");
    assert_eq!(human_age(600), "10m");
    assert_eq!(human_age(7200), "2h");
    assert_eq!(human_age(200_000), "2d");
}

#[test]
fn the_projection_query_stays_in_step_with_yupanas() {
    // Drift here means bobbin tells an agent about a boundary yupana does not
    // enforce, or the reverse. The shared shape is: an aegis:Policy at
    // boundary "action" with appliesTo, selector/predicate pulled so the
    // decode can tell a tripwire from a rule policy.
    for needle in [
        "aegis:boundary \\\"action\\\"",
        "aegis:appliesTo ?appliesTo",
        "OPTIONAL { ?policy aegis:selector ?selector }",
        "OPTIONAL { ?policy aegis:predicate ?predicate }",
        "OPTIONAL { ?policy aegis:backoffFormula ?backoffFormula }",
    ] {
        let unescaped = needle.replace("\\\"", "\"");
        assert!(
            TRIPWIRE_QUERY.contains(&unescaped),
            "projection query lost `{unescaped}`"
        );
    }
    // Bobbin's one addition over yupana's query.
    assert!(TRIPWIRE_QUERY.contains("aegis:claim ?claim"));
}
