# Injection Context Quality Assessment

_Strider patrol — 2026-02-22 (updated 2026-02-26)_

## Goal

Push bobbin closer to injection context targets for eventual publication of findings.
Directive from Hammond via Stiwi. No heavy eval runs yet — focus on quality improvements.

## Current Baseline (Stale)

Last eval run predates major quality improvements. Numbers below are **outdated**:

| Metric | no-bobbin | with-bobbin | Delta |
|--------|:---------:|:-----------:|:-----:|
| Test Pass Rate | 58.3% | 58.3% | — |
| Avg Precision | 83.3% | 88.9% | +5.6pp |
| Avg Recall | 51.4% | 54.2% | +2.8pp |
| Avg F1 | 61.7% | 64.8% | +3.1pp |

**Problem**: These results predate file classification, git blame bridging, query
preprocessing, recency boosting, and doc demotion. The real current delta is unknown.

## Implemented Quality Features (All Shipped)

| Feature | Status | Expected Impact |
|---------|--------|-----------------|
| FileCategory + classify_file() | ✅ Done | Fixes doc dominance (was 84% changelogs) |
| Git blame doc→source bridging | ✅ Done | Converts doc matches to source file finds |
| Gate threshold (0.50 default) | ✅ Done | Lowered from 0.75→0.50 (0.75 gated everything) |
| Session dedup | ✅ Done | Reduces redundant injection (hook_state.json) |
| Hot topics generation | ✅ Done | Auto-generates hot-topics.md every 10 injections |
| Sectioned output (Source/Docs) | ✅ Done | Source code prioritized in output |
| Query preprocessing | ✅ Done | Better keyword search from conversational prompts |
| Recency boosting | ✅ Done | Newer code ranked higher |
| Doc demotion (0.5x in RRF) | ✅ Done | Halves doc/config scores in ranking |
| Configurable search weights | ✅ Done | Enables calibration |
| Calibration tool (Rust-native) | ✅ Done | `bobbin calibrate` + `--full` sweep |
| Auto-calibration on index | ✅ Done | CalibrationGuard auto-triggers after `bobbin index` |
| Calibration status in `bobbin status` | ✅ Done | Shows config, F1, staleness, git profile |
| Eval framework improvements | ✅ Done | Token capture, gate details, agent guidance |

## What Moves the Needle Most

### Tier 1: Calibration — COMPLETE

**1. Search weight calibration** — ✅ DONE (Rust-native `bobbin calibrate`)

Replaced Python calibrate.py with Rust-native implementation. Auto-runs after
`bobbin index` via CalibrationGuard. Real-world sweep results (2026-02-26):

| Repo | Chunks | Best sw | Best F1 | Default F1 | Lift |
|------|-------:|--------:|--------:|-----------:|-----:|
| Flask | 7,171 | 0.50 | 0.082 | 0.061 | +33% |
| Ruff | 63,462 | 0.00 | 0.125 | 0.065 | +92% |
| Bobbin | 5,103 | 0.90 | 0.094 | 0.094 | +0% |

Full results: `docs/plans/calibration-sweep-results.md`

**2. Gate threshold** — ✅ DONE (lowered from 0.75 → 0.50)

Default 0.75 gated ALL queries. Now 0.50. Configurable per-repo.

### Tier 2: Injection Pipeline Refinements

**3. Budget allocation between source and docs**

Current budget is 300 lines. With dedup active, increasing search_limit (currently
hardcoded at 20 in hook.rs) is low-risk and could improve recall. Budget or
search_limit increase is the next lever.

**4. Coupling expansion quality**

`bobbin calibrate --full` now sweeps coupling_depth=[500, 2000, 5000, 20000].
Sweep running on flask (2026-02-26), results pending.

### Tier 3: Core Quality (Larger Effort)

**5. Embedding model upgrade**

all-MiniLM-L6-v2 is 23M params, 384 dimensions. Larger models offer better
semantic discrimination:
- all-MiniLM-L12-v2: same architecture, deeper (12 layers vs 6)
- BGE-small-en-v1.5: 33M params, designed for retrieval
- GTE-small: comparable size, strong on code

Would require re-indexing and testing. Medium effort, potentially high impact.

**6. Chunk boundary refinement**

Current AST-based chunking is good but could be improved for specific patterns:
- Function-level chunks might be too large for long functions
- Cross-function relationships (caller/callee) not captured in single chunks
- Comment-to-code association could be tighter

## Recommended Sequence (updated 2026-02-26)

1. ~~**Now**: Run calibration sweep~~ — ✅ DONE (Rust-native, auto-runs on index)
2. ~~**Next**: Gate threshold tuning~~ — ✅ DONE (lowered to 0.50)
3. **Now**: Increase search_limit for more candidate files (dedup keeps token cost low)
4. **Next**: Fresh agent eval run with calibrated configs to measure experience delta
5. **Then**: Adjust budget allocation based on eval data
6. **Later**: Embedding model upgrade as a major quality step
7. **Publication prep**: Full eval run with optimized config, comparison tables

## PO Targets (from ian, 2026-02-23)

| Metric | Target | Current (stale) | Gap |
|--------|--------|-----------------|-----|
| Precision | >= 85% | 88.9% | May already meet (need fresh data) |
| Recall | >= 65% | 54.2% | -10.8pp (biggest gap) |
| F1 | >= 72% | 64.8% | -7.2pp |
| Gate pass rate | >= 50% | ~~0% at 0.75~~ improved at 0.50 (need fresh data) |

**Key insight from ian**: Measure injection quality separately from task completion.
Injection quality = did bobbin surface the right files? We control injection, not the agent.

**Critical gap**: These numbers are stale. With gate threshold fix, calibration,
dedup, and all quality features now shipped, the true current numbers are unknown.
A fresh eval run is the highest-priority next step to validate improvements.
