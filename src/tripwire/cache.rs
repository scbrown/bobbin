//! Fetching the governed tripwire projection, and the cache that keeps it off
//! the hook's hot path.
//!
//! `bobbin inject-context` runs on **every** user prompt. The existing comment
//! in `src/cli/hook.rs` states the constraint plainly — "so Quipu latency never
//! enters this hook's hot path" — and a governance surface that adds a network
//! round trip to every keystroke would be removed within a week, which is a
//! worse outcome than a cache.
//!
//! So: a durable projection cache next to the rest of bobbin's state, with a
//! short TTL. This mirrors yupana's `projection_cache` and inherits its rule —
//! **a projection failure degrades to last-known wires, stale and SAYING SO,
//! never to wires silently vanishing.** The staleness is carried in
//! [`Provenance`] and rendered into the section; it is not an implementation
//! detail the reader has to infer.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{decode_tripwires, GovernedTripwire, Provenance};

/// The projection query.
///
/// Deliberately the same shape as yupana's `TRIPWIRE_QUERY`
/// (`src/project_queries.rs`) so the two consumers cannot drift into
/// disagreeing about what a tripwire *is* — bobbin telling an agent about a
/// boundary yupana does not enforce, or vice versa, would be worse than either
/// surface alone.
///
/// One addition: `aegis:claim`. Yupana does not project it because it composes
/// its model-facing message from the local wire's own fields. Bobbin has no
/// local wire and nothing to compose from — the claim IS the explanation, and
/// without it the agent gets a glob and an effect and no reason.
pub const TRIPWIRE_QUERY: &str = "\
PREFIX aegis: <http://aegis.gastown.local/ontology/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?policy ?name ?appliesTo ?effect ?claim ?constraintClass ?verificationPoint
       ?backoffFormula ?selector ?predicate WHERE {
  ?policy a aegis:Policy ;
          aegis:boundary \"action\" ;
          aegis:appliesTo ?appliesTo .
  OPTIONAL { ?policy aegis:selector ?selector }
  OPTIONAL { ?policy aegis:predicate ?predicate }
  OPTIONAL { ?policy rdfs:label ?name }
  OPTIONAL { ?policy aegis:effect ?effect }
  OPTIONAL { ?policy aegis:claim ?claim }
  OPTIONAL { ?policy aegis:constraintClass ?constraintClass }
  OPTIONAL { ?policy aegis:verificationPoint ?verificationPoint }
  OPTIONAL { ?policy aegis:backoffFormula ?backoffFormula }
}";

/// How long a cached projection is served without a refresh attempt.
///
/// Five minutes. Long enough that a busy session makes one quipu call, short
/// enough that re-scoping a wire reaches agents inside a coffee break. Quipu
/// treats an `appliesTo` write as governance-defining and invalidates its own
/// registry immediately; this is the lag bobbin adds on top, and it is stated
/// in the rendered section rather than assumed harmless.
const TTL_SECS: u64 = 300;

/// Timeout for the quipu read. Matches the search path's spotlight call.
const TIMEOUT_SECS: u64 = 2;

/// Resolve the quipu endpoint the same way the MCP tools do.
///
/// `BOBBIN_QUIPU_REMOTE` first (the testing override), then the config's
/// `quipu_endpoint`. `None` means this deployment has no governance plane, and
/// the caller must then do nothing at all — no call, no section, no cost.
#[must_use]
pub fn endpoint(config: &crate::config::Config) -> Option<String> {
    std::env::var("BOBBIN_QUIPU_REMOTE")
        .ok()
        .or_else(|| config.quipu_endpoint.clone())
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

/// Where the projection cache lives.
#[must_use]
pub fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".bobbin").join("tripwire-cache.json")
}

/// Seconds since the epoch, or 0 if the clock is before it.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The on-disk form. Plain JSON: the projection is small, and a format a human
/// can read is a format a human can check when a boundary looks wrong.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    endpoint: String,
    fetched_at: u64,
    /// The raw SPARQL body, not the decoded wires.
    ///
    /// Caching the *response* rather than the *decode* means a bobbin upgrade
    /// that learns to read a new field starts reading it from the existing
    /// cache, instead of serving a decode made by the older binary until the
    /// TTL happens to expire.
    body: String,
}

/// Load the governed tripwires, refreshing from quipu when the cache is cold.
///
/// Returns `None` when there is no endpoint configured — an ungoverned
/// deployment, where the honest output is nothing at all rather than a section
/// announcing its own absence on every prompt.
///
/// Returns `Some((wires, provenance))` otherwise, INCLUDING when the refresh
/// failed and a stale cache was used. The failure rides in the provenance and
/// is rendered; it is never swallowed, because "quipu was unreachable" and
/// "there are no wires here" are different facts and an agent that confuses
/// them walks into a boundary believing it checked.
pub async fn load_tripwires(
    config: &crate::config::Config,
    repo_root: &Path,
) -> Option<(Vec<GovernedTripwire>, Provenance)> {
    let endpoint = endpoint(config)?;
    let path = cache_path(repo_root);
    let cached = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<CacheFile>(&s).ok())
        .filter(|c| c.endpoint == endpoint);

    let now = now_secs();
    if let Some(ref c) = cached {
        let age = now.saturating_sub(c.fetched_at);
        if age < TTL_SECS {
            if let Ok(wires) = decode_tripwires(&c.body) {
                return Some((
                    wires,
                    Provenance::Cached {
                        endpoint,
                        age_secs: age,
                        refresh_error: None,
                    },
                ));
            }
        }
    }

    match fetch(&endpoint).await {
        Ok(body) => match decode_tripwires(&body) {
            Ok(wires) => {
                store(&path, &endpoint, now, &body);
                Some((wires, Provenance::Live { endpoint }))
            }
            Err(e) => degrade(
                cached,
                endpoint,
                now,
                format!("undecodable response: {e:#}"),
            ),
        },
        Err(e) => degrade(cached, endpoint, now, format!("{e:#}")),
    }
}

/// Fall back to the last-known projection, carrying the reason forward.
fn degrade(
    cached: Option<CacheFile>,
    endpoint: String,
    now: u64,
    reason: String,
) -> Option<(Vec<GovernedTripwire>, Provenance)> {
    let c = cached?;
    let wires = decode_tripwires(&c.body).ok()?;
    Some((
        wires,
        Provenance::Cached {
            endpoint,
            age_secs: now.saturating_sub(c.fetched_at),
            refresh_error: Some(reason),
        },
    ))
}

/// Best-effort persist. A cache that cannot be written is a performance
/// problem, not a correctness one, so a failure here is silent by design.
fn store(path: &Path, endpoint: &str, fetched_at: u64, body: &str) {
    let file = CacheFile {
        endpoint: endpoint.to_string(),
        fetched_at,
        body: body.to_string(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::write(path, json);
    }
}

/// POST the projection query to quipu's SPARQL endpoint.
async fn fetch(endpoint: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()?;
    let resp = client
        .post(format!("{endpoint}/query"))
        .json(&serde_json::json!({ "query": TRIPWIRE_QUERY }))
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "quipu /query returned HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        );
    }
    Ok(text)
}
