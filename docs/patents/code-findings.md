# Code findings from the patent-specification adversarial review

**Status:** pending import into beads. The shared beads database is at schema
v26 while the current `bd` binary expects v65; writes are blocked until a
designated machine migrates (`bd migrate --force && bd dolt push`). Import
these as bug beads once the schema is reconciled. Found by adversarial
doc-vs-code review (Claude agent session, 2026-08-14).

## 1. Stale grid-count comment on `run_full_sweep` (P3, docs)

The doc comment at `src/cli/calibrate.rs:1253-1255` claims "240 configs …
960 configs … 19,200 probes", but omits the budget (×3), search-limit (×4),
and bridge mode/factor (×8) dimensions the function actually sweeps via
`build_grid_with_recency` (`calibrate.rs:398-458`, `1282-1287`). The real
geometry is 23,040 configs per coupling depth, 92,160 total, ~1.84M probes at
default samples — matching the runtime's own printout at
`calibrate.rs:1291-1303`. Update the comment or derive it from the constants.

## 2. Intent doc-demotion factor has opposite semantics in local vs remote hook paths (P1, bug)

The remote path inverts into effect space before applying the intent factor
(`src/cli/hook.rs:1082-1090`: `effect = (1 - dd) * factor; dd' = 1 - effect`),
so BugFix's 1.5 factor strengthens demotion as intended. The local path
raw-multiplies the score multiplier (`src/cli/hook.rs:2873`:
`(base_dd * adj.doc_demotion_factor).clamp(0.01, 1.0)`), so the same 1.5
factor raises the multiplier and weakens demotion — the exact opposite
effect. The local path should adopt the effect-space form.

## 3. Complementary expansion dedup misses non-adjacent duplicates (P2, bug)

`src/cli/hook.rs:3158-3159` sorts complementary candidates by coupling score
descending, then calls `dedup_by(|a, b| a.0 == b.0)`, which only removes
consecutive equal-path entries. A file reached via multiple seen files with
different coupling scores survives as duplicate entries and can consume
several of the 5 truncated slots. Dedupe by path (keep max score per path)
before sorting/truncating.

## 4. Topic fingerprint truncates top-10 alphabetically, not by score (P3, bug-or-document)

`compute_session_id` (`src/cli/hook.rs:2273-2293`) collects above-threshold
chunk keys, sorts alphabetically, then truncates to 10 before hashing. Which
chunks enter the fingerprint therefore depends on path lexicography, not
relevance. If intentional, document it; if not, sort by score, truncate, then
sort keys for hash stability.

## 5. Failure and post-tool-use injections carry no injection IDs and bypass the session ledger (P2, feature gap)

Only prompt-submission paths generate injection identifiers and store
injection records (`src/cli/hook.rs:1450/1509`, `3219/3234-3246`); the
failure-handler's semantic fallback passes `None` (`src/cli/hook.rs:4672`)
and the parse-directed and post-tool-use paths store nothing, so those
injections cannot be rated via feedback. They also neither filter against nor
record to the `SessionLedger` (consulted only at `hook.rs:1164-1176/1481` and
`3089-3110/3274-3276`), so they can re-inject chunks the prompt path already
delivered, invisible to delta filtering. Extend the ID scheme and ledger
integration to all injecting event classes.
