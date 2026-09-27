# Evaluation v2: results and study status

**Partial snapshot — 25 recorded L1 cells; no final results published.**

This page replaces the v1 aggregate with the preregistered v2 reporting structure.
The continuation was still running at the snapshot below. Final tables remain
withheld until it finishes and its completion report has been reviewed and recorded.
A completed pilot will remain descriptive; it will not establish effectiveness.

The [historical v1 tables](summary-v1.md) are preserved for audit. They mix model
attribution, tool availability and automatic injection, and must not be presented
as v2 results or evidence that Bobbin improves task success.

## What v2 measures

- **L0, offline retrieval:** hunk recall and useful-line density at a fixed line
  budget, with file recall/precision, injected volume and error/skip counts.
- **L1, agent pilot:** a paired 2×2 design separates search-tool access from
  automatic injection. Hidden-test task success is primary; file F1 is secondary.
- **L2, broader benchmark:** planned, with no results reported here.

The L1 cells are `none` (neither), `tool` (search only), `inject` (injection only)
and `both`. Every intended task has four cells. A result file alone does not prove
valid execution, and a stream without a result does not count as a completed cell.

See the [preregistration](https://github.com/scbrown/bobbin/blob/main/eval/v2/PREREGISTRATION.md)
and [recovery contract](https://github.com/scbrown/bobbin/blob/main/eval/v2/PILOT_RECOVERY.md)
for the frozen design and dated deviations.

## Partial L1 coverage — 25 recorded cells

Snapshot: **2026-09-27T03:44:14.862605+00:00**. Counts come from the
[frozen coverage artifact](https://github.com/scbrown/bobbin/blob/main/eval/v2/reports/partial-20260927.json),
replayed from stored results without model calls. They will not update automatically.

| Source (partial) | Recorded cells | Agent-valid and test-valid cells | Tasks with all four result files | Tasks with all four cells valid |
| --- | ---: | ---: | ---: | ---: |
| Original interrupted pilot | 9 | 8 | 2 | 2 |
| Bounded continuation, 20 cells approved | 16 | 15 | 4 | 3 |
| Combined coverage snapshot | 25 | 23 | 6 | 5 |

**These validity counts are diagnostics, not a replacement denominator or a
post-hoc exclusion rule.** The completion report must classify invalid execution
and apply the registered outcome policy before any success-rate table is published.

- `nushell-005` has only one of four original results and remains incomplete.
- `typst-002` has four continuation results, but its `inject` cell is invalid.
  Four files must not be described as four valid measurements.
- At this snapshot, one continuation stream has no result. Its completion and
  cost are unknown; neither is assigned zero.
- Eight original tasks failed fixture controls. Of the 19 untouched tasks checked
  for recovery, 14 failed the discriminating-fixture requirement; five qualified
  for the bounded continuation. These exclusions are separate from agent failures.
- The original plan was 30 tasks × four cells (120 cells). Neither this partial
  snapshot nor the bounded continuation constitutes that completed pilot.
- Original and continuation labels stay separate. The continuation pins its
  client; the original client identity remains unpinned.

## Partial L0 coverage — 858 task/arm/budget records

The offline campaign stopped before the full task set completed. The frozen file
contains two manifest records: the first precedes no scores; the second identifies
all 858 scores. No task/arm/budget keys repeat. The replay is not a new experiment.

| Coverage or outcome (partial; 858 records) | Count |
| --- | ---: |
| Tasks with saved scores / planned tasks | 26 / 40 |
| Arms per recorded task | 11 |
| Line budgets per arm | 3 (100, 300, 600) |
| Injected | 569 |
| Skipped | 212 |
| Error | 77 |

An error is not a successful empty retrieval. Error and skip counts must accompany
quality metrics, with the actual paired sample size for each contrast. Empty
injections have undefined density. The registered comparison uses the 300-line
budget; the other budgets are secondary. Do not pool the three budgets as extra
independent tasks. No final L0 estimates or significance claims appear here.

## Final reporting structure

Once the completion gate is met, this page will add:

1. Run identity, model/client/runtime pins, task flow and all exclusions.
2. L0 paired hunk-recall and density contrasts versus `full`, with per-contrast
   sample sizes, confidence intervals and registered multiplicity correction.
3. L1 source-separated descriptive outcomes, invalid/incomplete counts, complete
   task contrasts, uncertainty, token usage, wall time and recorded cost coverage.
4. Deviations, fixture attrition, contamination limits and remaining unknowns.

The full L1 study is not approved or run. The pilot cannot support a causal uplift
headline, an ablation ranking, or a claim that the original task set was completed.

## Reproduction and companion draft

The [replay notes](https://github.com/scbrown/bobbin/blob/main/eval/v2/RESULTS.md)
explain the stored-output inputs and partial-publication gate. The
[v0.3 paper draft](https://github.com/scbrown/bobbin/blob/main/docs/paper-context-injection.md)
uses the same snapshot and withholds final findings. Older per-project pages and
[historical trends](trends.md) remain v1 material.
