//! Tests for `cross_repo`.
//!
//! Split out of `cross_repo.rs` so that file clears the 500-line error limit
//! (bobbin-aoz). `scripts/check-file-size.sh` exempts `*tests.rs` by
//! design, and the alternative — an allowlist entry — is the exit that
//! makes the ratchet meaningless.

use super::*;
use crate::config::{AccessConfig, RoleConfig};

fn bead_map(entries: &[(&str, &[&str], i64)]) -> BeadFileMap {
    let mut m = BeadFileMap::new();
    for (bead, files, ts) in entries {
        m.insert(
            bead.to_string(),
            (files.iter().map(|s| s.to_string()).collect(), *ts),
        );
    }
    m
}

/// AC#2: a bead present in two repos yields a canonical cross-repo pair; no
/// same-repo pairs are ever emitted.
#[test]
fn pairs_shared_bead_across_two_repos() {
    let repos = vec![
        (
            "api".to_string(),
            bead_map(&[("bo-1", &["contract.rs", "other.rs"], 100)]),
        ),
        (
            "web".to_string(),
            bead_map(&[("bo-1", &["client.ts"], 200)]),
        ),
    ];
    let pairs = pair_cross_repo(&repos, 0.7, 30.0, 1000);
    // 2 files in api x 1 in web = 2 cross-repo pairs; zero same-repo pairs.
    assert_eq!(pairs.len(), 2);
    for p in &pairs {
        assert_ne!(p.repo_a, p.repo_b, "no same-repo pairs");
        assert_eq!(p.co_changes, 1);
        assert_eq!(p.last_co_change, 200, "max timestamp across the two sides");
    }
    // Canonical ordering: (api, ...) sorts before (web, ...).
    assert!(pairs.iter().all(|p| p.repo_a == "api" && p.repo_b == "web"));
}

/// AC#2/#3: a bead present in only one repo of the set produces no edge, and
/// repos NOT passed in (i.e. another group) are never paired — structural
/// group gating.
#[test]
fn no_edge_for_single_repo_bead_or_repo_outside_set() {
    // `bo-solo` lives only in `api`; `bo-shared` is shared api<->web.
    // `infra` belongs to a different group and is simply not passed here.
    let repos = vec![
        (
            "api".to_string(),
            bead_map(&[("bo-solo", &["only.rs"], 50), ("bo-shared", &["a.rs"], 60)]),
        ),
        ("web".to_string(), bead_map(&[("bo-shared", &["b.ts"], 70)])),
    ];
    let pairs = pair_cross_repo(&repos, 0.7, 30.0, 1000);
    assert_eq!(pairs.len(), 1, "only the shared bead couples");
    assert_eq!(pairs[0].path_a, "a.rs");
    assert_eq!(pairs[0].path_b, "b.ts");
    // `only.rs` must never appear — it is single-repo.
    assert!(pairs
        .iter()
        .all(|p| p.path_a != "only.rs" && p.path_b != "only.rs"));
}

/// A bead spanning three in-group repos couples every distinct repo pair.
#[test]
fn three_repo_bead_pairs_each_distinct_repo_pair() {
    let repos = vec![
        ("a".to_string(), bead_map(&[("bo-x", &["fa.rs"], 10)])),
        ("b".to_string(), bead_map(&[("bo-x", &["fb.rs"], 20)])),
        ("c".to_string(), bead_map(&[("bo-x", &["fc.rs"], 30)])),
    ];
    let pairs = pair_cross_repo(&repos, 0.7, 30.0, 1000);
    // a-b, a-c, b-c
    assert_eq!(pairs.len(), 3);
}

fn access(default_allow: bool, roles: Vec<RoleConfig>) -> AccessConfig {
    AccessConfig {
        default_allow,
        roles,
    }
}

fn deny_role(name: &str, deny: &[&str]) -> RoleConfig {
    RoleConfig {
        name: name.to_string(),
        allow: vec![],
        deny: deny.iter().map(|s| s.to_string()).collect(),
        deny_paths: vec![],
    }
}

/// AC#5 (BLOCKING, sentinel-reviewed): deny-contrast. A role that denies
/// `pixelsrc` must NOT receive pixelsrc files via a cross-repo edge, while an
/// allow-all role does. Mirrors the bead pixelsrc-deny test in access.rs.
#[test]
fn access_filter_blocks_denied_repo_on_cross_repo_edge() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let store = MetadataStore::open(&db).unwrap();
    // Seed file in `aegis` is coupled to a file in `pixelsrc`.
    store
        .upsert_cross_repo_coupling(&CrossRepoCoupling {
            repo_a: "aegis".to_string(),
            path_a: "src/seed.rs".to_string(),
            repo_b: "pixelsrc".to_string(),
            path_b: "src/secret.rs".to_string(),
            score: 0.9,
            co_changes: 3,
            last_co_change: 1000,
        })
        .unwrap();

    // Allow-all: the pixelsrc file IS surfaced.
    let permissive = RepoFilter::allow_all();
    let allowed =
        related_cross_repo(&store, Some("aegis"), "src/seed.rs", 10, 0.0, &permissive).unwrap();
    assert_eq!(allowed.len(), 1);
    assert_eq!(allowed[0].repo, "pixelsrc");
    assert_eq!(allowed[0].path, "src/secret.rs");

    // Role denying pixelsrc: the edge leaks NOTHING.
    let cfg = access(true, vec![deny_role("aegis", &["pixelsrc"])]);
    let denying = RepoFilter::from_config(&cfg, "aegis");
    let denied =
        related_cross_repo(&store, Some("aegis"), "src/seed.rs", 10, 0.0, &denying).unwrap();
    assert!(
        denied.is_empty(),
        "denied repo must not leak via coupling edge"
    );
}

/// bo-4t07 (defense-in-depth): an edge whose OTHER side has an empty repo must
/// surface to NO role — not even allow-all. Without the guard, the synthetic
/// `repos//path` collapses to repo "" which slips through `is_allowed("")` under
/// default_allow=true (fail-open for deny-list roles).
#[test]
fn empty_repo_edge_never_surfaces_even_to_allow_all() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("meta.db");
    let store = MetadataStore::open(&db).unwrap();
    // Seed in `aegis` coupled to a file whose repo is empty (malformed edge).
    store
        .upsert_cross_repo_coupling(&CrossRepoCoupling {
            repo_a: "aegis".to_string(),
            path_a: "src/seed.rs".to_string(),
            repo_b: "".to_string(),
            path_b: "src/secret.rs".to_string(),
            score: 0.9,
            co_changes: 3,
            last_co_change: 1000,
        })
        .unwrap();

    // Even the most permissive filter must drop the empty-repo side.
    let permissive = RepoFilter::allow_all();
    let out =
        related_cross_repo(&store, Some("aegis"), "src/seed.rs", 10, 0.0, &permissive).unwrap();
    assert!(
        out.is_empty(),
        "empty-repo edge must never surface (fail-closed), even to allow-all"
    );
}

/// AC#6: end-to-end over two real temp git repos sharing a bead trailer ->
/// the compute pass stores an edge and `related` surfaces the cross-repo file.
#[test]
fn integration_two_temp_repos_share_bead_trailer() {
    use std::process::Command;

    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir)
            .output()
            .unwrap();
    }
    fn commit(dir: &Path, file: &str, body: &str, msg: &str) {
        std::fs::write(dir.join(file), body).unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    let api_dir = tempfile::tempdir().unwrap();
    let web_dir = tempfile::tempdir().unwrap();
    init_repo(api_dir.path());
    init_repo(web_dir.path());
    // Both commits carry the same bead trailer `Bead: bo-share`.
    commit(
        api_dir.path(),
        "contract.rs",
        "v1",
        "add contract\n\nBead: bo-share",
    );
    commit(
        web_dir.path(),
        "client.ts",
        "v1",
        "consume contract\n\nBead: bo-share",
    );

    let store_dir = tempfile::tempdir().unwrap();
    let db = store_dir.path().join("meta.db");
    let store = MetadataStore::open(&db).unwrap();
    store
        .set_meta("repo_source:api", &api_dir.path().to_string_lossy())
        .unwrap();
    store
        .set_meta("repo_source:web", &web_dir.path().to_string_lossy())
        .unwrap();

    // Config with both repos in one group.
    let mut config = Config::default();
    config.groups = vec![crate::config::GroupConfig {
        name: "svc".to_string(),
        repos: vec!["api".to_string(), "web".to_string()],
    }];

    let stored = compute_and_store_cross_repo(&store, &config).unwrap();
    assert_eq!(stored, 1, "one cross-repo edge from the shared bead");

    // `related` on the api file surfaces the web file, annotated with its repo.
    let rel = related_cross_repo(
        &store,
        Some("api"),
        "contract.rs",
        10,
        0.0,
        &RepoFilter::allow_all(),
    )
    .unwrap();
    assert_eq!(rel.len(), 1);
    assert_eq!(rel[0].repo, "web");
    assert_eq!(rel[0].path, "client.ts");
}

/// A bead spanning two *different groups* creates no cross-group edge, because
/// each group is computed from its own repo set only.
#[test]
fn integration_no_cross_group_edge() {
    use std::process::Command;
    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir)
            .output()
            .unwrap();
    }
    fn commit(dir: &Path, file: &str, msg: &str) {
        std::fs::write(dir.join(file), "v1").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(dir)
            .output()
            .unwrap();
    }
    let g1 = tempfile::tempdir().unwrap();
    let g2 = tempfile::tempdir().unwrap();
    init_repo(g1.path());
    init_repo(g2.path());
    // Same bead `bo-span` touches a repo in group1 and a repo in group2.
    commit(g1.path(), "one.rs", "g1 work\n\nBead: bo-span");
    commit(g2.path(), "two.rs", "g2 work\n\nBead: bo-span");

    let sd = tempfile::tempdir().unwrap();
    let store = MetadataStore::open(&sd.path().join("m.db")).unwrap();
    store
        .set_meta("repo_source:r1", &g1.path().to_string_lossy())
        .unwrap();
    store
        .set_meta("repo_source:r2", &g2.path().to_string_lossy())
        .unwrap();

    let mut config = Config::default();
    config.groups = vec![
        crate::config::GroupConfig {
            name: "group1".into(),
            repos: vec!["r1".into()],
        },
        crate::config::GroupConfig {
            name: "group2".into(),
            repos: vec!["r2".into()],
        },
    ];
    // Each group has <2 resolvable repos -> no edges at all.
    let stored = compute_and_store_cross_repo(&store, &config).unwrap();
    assert_eq!(
        stored, 0,
        "no cross-group edge: r1 and r2 are in different groups"
    );
}
