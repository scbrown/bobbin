//! Decoding quipu's SPARQL result rows into [`GovernedTripwire`]s.
//!
//! The row shape is SPARQL 1.1 JSON results: `results.bindings` is an array of
//! objects keyed by variable name, each value an object with a `value` field.
//!
//! Two multiplicities have to be kept straight, and getting them backwards is
//! the whole reason this is a module and not an inline `map`:
//!
//! - `appliesTo` is **genuinely multi-valued**. A policy scoped to three globs
//!   arrives as three rows and the globs ACCUMULATE. Keeping whichever glob
//!   arrived first would silently shrink a boundary — the failure quipu's own
//!   governance doc calls out by name.
//! - Everything else is single-valued. If two rows for one policy disagree,
//!   that is a conflicting definition, and the wire is marked conflicted
//!   rather than resolved by row order (see [`GovernedTripwire::conflicts`]).

use super::{GovernedTripwire, TripEffect};

/// Decode a SPARQL JSON result body into governed tripwires.
///
/// Rows binding `selector` or `predicate` are **rule** policies, not
/// tripwires: quipu's definition of a tripwire is a path-boundary policy with
/// neither. They are skipped, and skipping them is not an error — the rule
/// plane is a different consumer's business.
///
/// Returns `Err` only when the payload is not a SPARQL result set at all. A
/// malformed *wire* is never an error: it comes back marked (rule 2).
pub fn decode_tripwires(sparql_json: &str) -> anyhow::Result<Vec<GovernedTripwire>> {
    let value: serde_json::Value =
        serde_json::from_str(sparql_json).map_err(|e| anyhow::anyhow!("not JSON: {e}"))?;
    let rows = value
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
        .ok_or_else(|| anyhow::anyhow!("no results.bindings array in SPARQL response"))?;

    let mut order: Vec<String> = Vec::new();
    let mut acc: std::collections::HashMap<String, GovernedTripwire> =
        std::collections::HashMap::new();
    let mut conflicts: Vec<&'static str> = Vec::new();

    for row in rows {
        let get = |key: &str| -> Option<String> {
            row.get(key)
                .and_then(|b| b.get("value"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };
        // A selector or predicate makes it a rule policy, enforced elsewhere.
        if get("selector").is_some() || get("predicate").is_some() {
            continue;
        }
        let Some(iri) = get("policy") else { continue };
        let Some(glob) = get("appliesTo") else {
            // No boundary means no tripwire. Not a defect to report: it is a
            // policy of a different kind that this query happened to touch.
            continue;
        };

        if !order.contains(&iri) {
            order.push(iri.clone());
            acc.insert(
                iri.clone(),
                GovernedTripwire {
                    policy: iri.clone(),
                    name: get("name").unwrap_or_else(|| last_segment(&iri).to_string()),
                    paths: Vec::new(),
                    effect: TripEffect::parse(get("effect").as_deref()),
                    claim: get("claim"),
                    class: get("constraintClass"),
                    verification_point: get("verificationPoint"),
                    backoff_formula: get("backoffFormula"),
                    conflicts: Vec::new(),
                },
            );
        }
        let wire = acc.get_mut(&iri).expect("inserted above");
        if !wire.paths.contains(&glob) {
            wire.paths.push(glob);
        }

        // Single-valued fields: agree, or be marked.
        let effect = TripEffect::parse(get("effect").as_deref());
        if effect != TripEffect::Undeclared && effect != wire.effect {
            if wire.effect == TripEffect::Undeclared {
                wire.effect = effect;
            } else {
                conflicts.push("aegis:effect");
            }
        }
        for (slot, key, incoming) in [
            (&mut wire.claim, "aegis:claim", get("claim")),
            (
                &mut wire.class,
                "aegis:constraintClass",
                get("constraintClass"),
            ),
            (
                &mut wire.verification_point,
                "aegis:verificationPoint",
                get("verificationPoint"),
            ),
            (
                &mut wire.backoff_formula,
                "aegis:backoffFormula",
                get("backoffFormula"),
            ),
        ] {
            match (&slot, incoming) {
                (Some(existing), Some(v)) if **existing != v => conflicts.push(key),
                (None, Some(v)) => *slot = Some(v),
                _ => {}
            }
        }
        for field in conflicts.drain(..) {
            if !wire.conflicts.iter().any(|c| c == field) {
                wire.conflicts.push(field.to_string());
            }
        }
    }

    Ok(order
        .into_iter()
        .filter_map(|iri| acc.remove(&iri))
        .collect())
}

/// Last path segment of an IRI, for naming a policy that carries no label.
fn last_segment(iri: &str) -> &str {
    iri.rsplit(['/', '#']).next().unwrap_or(iri)
}
