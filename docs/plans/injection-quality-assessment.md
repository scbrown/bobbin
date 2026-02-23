# Injection Context Quality Assessment

_Strider patrol — 2026-02-22_

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
| Gate threshold (0.75 default) | ✅ Done | Suppresses irrelevant injection |
| Session dedup | ✅ Done | Reduces redundant injection |
| Sectioned output (Source/Docs) | ✅ Done | Source code prioritized in output |
| Query preprocessing | ✅ Done | Better keyword search from conversational prompts |
| Recency boosting | ✅ Done | Newer code ranked higher |
| Doc demotion (0.5x in RRF) | ✅ Done | Halves doc/config scores in ranking |
| Configurable search weights | ✅ Done | Enables calibration |
| Calibration tool | ✅ Done | Sweep params without LLM calls |
| Eval framework improvements | ✅ Done | Token capture, gate details, agent guidance |

## What Moves the Needle Most

### Tier 1: Calibration (Highest Impact, Ready Now)

**1. Search weight calibration** — bobbin-24ui (in progress, assigned to dearing)

The calibrate.py tool can sweep semantic_weight, doc_demotion, and rrf_k across
eval tasks WITHOUT running LLM agents. Pure search quality measurement. This is
the fastest path to improved defaults.

Current defaults may not be optimal:
- semantic_weight=0.7 — is 0.8 better for code tasks?
- doc_demotion=0.5 — is 0.3 or 0.2 needed to push source files to the top?
- rrf_k=60 — standard value, but lower k (30-40) might benefit top-heavy results

**2. Gate threshold production tuning**

Default 0.75 is aggressive. Flask eval showed ALL queries were gate-skipped at
this threshold. For agent use cases (where queries are conversational), 0.50-0.60
may be the sweet spot. The calibration tool can test this.

### Tier 2: Injection Pipeline Refinements

**3. Budget allocation between source and docs**

With show_docs=true (default), docs compete with source code for the 300-line
budget. Consider a split budget: e.g., 240 lines source, 60 lines docs. Or
increase total budget to 400+ given modern context windows.

**4. Coupling expansion quality**

Temporal coupling expansion adds files that co-change. It can help (ruff-001:
33.3% → 66.7% F1) or be noise. Need data on coupling hit rate vs. miss rate
across eval tasks. The calibration tool could measure this by comparing results
with --depth 0 vs --depth 1.

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

## Recommended Sequence

1. **Now**: Run calibration sweep (search weights, doc demotion, gate threshold)
2. **Next**: Light validation — spot-check 2-3 tasks with improved defaults
3. **Then**: Adjust budget allocation and coupling depth based on calibration data
4. **Later**: Embedding model upgrade as a major quality step
5. **Publication prep**: Full eval run with optimized config, comparison tables

## Coordination

- **seth** — fellow crew in bobbin, should coordinate on calibration analysis
- **dearing** — assigned to bobbin-24ui (calibration bead), external to bobbin rig
- **ian** — keeper, needs briefing on injection quality direction

## PO Targets (from ian, 2026-02-23)

| Metric | Target | Current (stale) | Gap |
|--------|--------|-----------------|-----|
| Precision | >= 85% | 88.9% | May already meet (need fresh data) |
| Recall | >= 65% | 54.2% | -10.8pp (biggest gap) |
| F1 | >= 72% | 64.8% | -7.2pp |
| Gate pass rate | >= 50% | ~0% at 0.75 | Gating everything — #1 problem |

**Key insight from ian**: Measure injection quality separately from task completion.
Injection quality = did bobbin surface the right files? We control injection, not the agent.

**Publication**: Too early to discuss format. Hit targets first.

## Priority Sequence (PO-approved)

1. **Gate threshold calibration** — sweep 0.50, 0.55, 0.60, 0.65 (current 0.75 gates ALL queries)
2. **Fresh eval run** with current defaults to establish true baseline
3. **Semantic weight + doc demotion sweep** to optimize ranking
4. **Commit tuned defaults** based on calibration data
