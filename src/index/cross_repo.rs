//! Cross-repo coupling via bead-reference co-occurrence (bo-oqny).
//!
//! The homelab is a monorepo-of-repos: an API contract changes in repo A and its
//! consumer changes in repo B, but separate git repos share no commits, so
//! per-repo coupling (`git.rs`) can never link them. This module infers the link
//! from **bead references**: a bead id appearing in the bead trailers of commits
//! in two repos of the same [`GroupConfig`](crate::config::GroupConfig) is one
//! logical change, so the files those commits touched are coupled across repos.
//!
//! Signal **(A) bead-reference co-occurrence ONLY** — temporal commit proximity
//! (B) is explicitly rejected as too noisy (ian ruling, bo-oqny).
//!
//! ## Security (BLOCKING)
//!
//! Cross-repo coupling is a net-new leak surface: it surfaces *other* repos'
//! files. Read-time `[access]` role filtering is therefore MANDATORY and is
//! enforced in one place — [`related_cross_repo`] — which every read surface
//! (CLI / MCP / HTTP `related`) routes through. A role that denies repo X must
//! not receive repo-X files via a coupling edge.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::Result;

use crate::access::RepoFilter;
use crate::config::Config;
use crate::index::git::{calculate_coupling_score, GitAnalyzer};
use crate::storage::MetadataStore;
use crate::types::CrossRepoCoupling;

/// Per-repo map of `bead_id -> (touched files, latest commit timestamp)`.
pub type BeadFileMap = HashMap<String, (BTreeSet<String>, i64)>;

/// A canonical `((repo, path), (repo, path))` endpoint pair (used as an accumulator key).
type PairKey = ((String, String), (String, String));

/// Cap on file pairs emitted per (bead, repo-pair) to bound a single noisy bead.
const MAX_PAIRS_PER_BEAD_REPO_PAIR: usize = 400;

/// A cross-repo file related to a seed file, after access filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossRepoRelated {
    /// Repo the related file lives in (annotation the caller surfaces).
    pub repo: String,
    /// Repo-relative path of the related file.
    pub path: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Order two `(repo, path)` endpoints canonically so a pair dedupes regardless
/// of which repo it was discovered from.
fn canonical<'a>(
    a: (&'a str, &'a str),
    b: (&'a str, &'a str),
) -> ((&'a str, &'a str), (&'a str, &'a str)) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Pure pairing: emit canonical cross-repo file pairs from each repo's
/// `bead_id -> files` map.
///
/// **Group gating is structural**: this function only ever pairs repos present
/// in `repos`, and never emits a same-repo pair. Callers enforce "no cross-group
/// edges" simply by passing one group's repos at a time — a bead that also
/// touches a repo outside this set produces no edge to it (see tests).
///
/// `now` is injected (not read from the clock) so scoring is deterministic and
/// unit-testable.
pub fn pair_cross_repo(
    repos: &[(String, BeadFileMap)],
    freq_weight: f32,
    recency_days: f32,
    now: i64,
) -> Vec<CrossRepoCoupling> {
    // Accumulate co-change counts and last timestamp per canonical pair.
    let mut acc: HashMap<PairKey, (u32, i64)> = HashMap::new();

    // Union of every bead id seen anywhere, so we only walk shared beads.
    let mut all_beads: BTreeSet<&str> = BTreeSet::new();
    for (_, map) in repos {
        for bead in map.keys() {
            all_beads.insert(bead.as_str());
        }
    }

    for bead in all_beads {
        // Which repos (index into `repos`) reference this bead?
        let present: Vec<usize> = repos
            .iter()
            .enumerate()
            .filter(|(_, (_, map))| map.contains_key(bead))
            .map(|(i, _)| i)
            .collect();
        if present.len() < 2 {
            continue; // bead lives in a single repo — no cross-repo signal
        }

        for ii in 0..present.len() {
            for jj in (ii + 1)..present.len() {
                let (repo_i, map_i) = &repos[present[ii]];
                let (repo_j, map_j) = &repos[present[jj]];
                // Distinct repos guaranteed (different indices, and a group must
                // not list the same repo twice). Guard anyway.
                if repo_i == repo_j {
                    continue;
                }
                let (files_i, ts_i) = &map_i[bead];
                let (files_j, ts_j) = &map_j[bead];
                let last = (*ts_i).max(*ts_j);

                let mut emitted = 0usize;
                'outer: for fi in files_i {
                    for fj in files_j {
                        let (lo, hi) = canonical((repo_i, fi), (repo_j, fj));
                        let key = (
                            (lo.0.to_string(), lo.1.to_string()),
                            (hi.0.to_string(), hi.1.to_string()),
                        );
                        let e = acc.entry(key).or_insert((0, 0));
                        e.0 += 1;
                        if last > e.1 {
                            e.1 = last;
                        }
                        emitted += 1;
                        if emitted >= MAX_PAIRS_PER_BEAD_REPO_PAIR {
                            break 'outer;
                        }
                    }
                }
            }
        }
    }

    let max_co = acc.values().map(|(c, _)| *c).max().unwrap_or(0);
    let mut out: Vec<CrossRepoCoupling> = acc
        .into_iter()
        .map(|((a, b), (co_changes, last_co_change))| CrossRepoCoupling {
            repo_a: a.0,
            path_a: a.1,
            repo_b: b.0,
            path_b: b.1,
            score: calculate_coupling_score(
                co_changes,
                max_co,
                last_co_change,
                now,
                freq_weight,
                recency_days,
            ),
            co_changes,
            last_co_change,
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Compute cross-repo coupling for every configured group and replace the stored
/// table (bo-oqny). Group-scoped: only repos sharing a [`GroupConfig`] are paired,
/// so a bead spanning two groups creates no cross-group edge.
///
/// Repos are resolved to source paths via the `repo_source:<name>` meta registry;
/// repos not yet indexed (no registry entry) or whose source is not a git repo are
/// silently skipped, so a group materializes once ≥2 of its repos are indexed.
/// Returns the number of edges stored.
pub fn compute_and_store_cross_repo(ms: &MetadataStore, config: &Config) -> Result<usize> {
    // Rebuild wholesale, mirroring per-repo coupling.
    ms.clear_cross_repo_coupling()?;
    if config.groups.is_empty() {
        return Ok(0);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut total = 0usize;
    ms.begin_transaction()?;
    let result = (|| -> Result<()> {
        for group in &config.groups {
            // Resolve each in-group repo's source path and build its bead->files map.
            let mut repos: Vec<(String, BeadFileMap)> = Vec::new();
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for repo_name in &group.repos {
                if !seen.insert(repo_name.as_str()) {
                    continue; // de-dup repos listed twice in a group
                }
                let src = match ms.get_meta(&format!("repo_source:{}", repo_name))? {
                    Some(s) => s,
                    None => continue,
                };
                let analyzer = match GitAnalyzer::new(Path::new(&src)) {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                let map = analyzer.bead_file_map(config.git.coupling_depth)?;
                if !map.is_empty() {
                    repos.push((repo_name.clone(), map));
                }
            }
            if repos.len() < 2 {
                continue;
            }
            let pairs = pair_cross_repo(
                &repos,
                config.git.coupling_freq_weight,
                config.git.coupling_recency_days,
                now,
            );
            for p in &pairs {
                ms.upsert_cross_repo_coupling(p)?;
                total += 1;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            ms.commit()?;
            Ok(total)
        }
        Err(e) => {
            let _ = ms.rollback();
            Err(e)
        }
    }
}

/// Fetch cross-repo coupled files for a seed `(repo, path)`, **access-filtered**.
///
/// This is the single security chokepoint for cross-repo reads (sentinel-reviewed,
/// bo-oqny AC#5). Every result is the file on the *opposite* side of the seed and
/// is dropped unless `filter.is_path_allowed` permits its repo — so a role denying
/// repo X never receives repo-X files via a coupling edge. `seed_repo` is `None`
/// when the caller cannot resolve the seed's repo (single-repo store); matching
/// then falls back to path alone, but the *result* is still access-filtered.
pub fn related_cross_repo(
    store: &MetadataStore,
    seed_repo: Option<&str>,
    seed_path: &str,
    limit: usize,
    threshold: f32,
    filter: &RepoFilter,
) -> Result<Vec<CrossRepoRelated>> {
    let edges = store.get_cross_repo_coupling(seed_repo, seed_path, limit)?;
    let mut out = Vec::new();
    for e in edges {
        if e.score < threshold {
            continue;
        }
        // Pick the side that is NOT the seed.
        let seed_is_a = e.path_a == seed_path && seed_repo.is_none_or(|r| r == e.repo_a);
        let (other_repo, other_path) = if seed_is_a {
            (e.repo_b, e.path_b)
        } else {
            (e.repo_a, e.path_a)
        };
        // SECURITY (defense-in-depth, bo-4t07): treat an empty/blank repo or path as
        // DENY. A synthetic `repos//path` would yield repo "" and slip through
        // `is_allowed("")` for deny-list roles under default_allow=true (fail-open).
        // Real data never emits empty-repo edges today, but enforce it here so a
        // future producer change cannot leak.
        if other_repo.trim().is_empty() || other_path.trim().is_empty() {
            continue;
        }
        // SECURITY: build a synthetic `repos/<repo>/<path>` so RepoFilter extracts
        // the correct repo AND applies any deny_paths to the repo-relative path.
        let synthetic = format!("repos/{}/{}", other_repo, other_path);
        if !filter.is_path_allowed(&synthetic) {
            continue;
        }
        out.push(CrossRepoRelated {
            repo: other_repo,
            path: other_path,
            score: e.score,
            co_changes: e.co_changes,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
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
}
