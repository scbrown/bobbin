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
#[path = "cross_repo_tests.rs"]
mod tests;
