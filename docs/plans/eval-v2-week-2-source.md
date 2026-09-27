# Week-2 evaluation update — source draft

**Draft, not a final results announcement.** Use the
[v2 book page](../book/src/eval/summary.md),
[v0.3 paper](../paper-context-injection.md) and
[frozen partial artifact](../../eval/v2/reports/partial-20260927.json) as one source.
Do not publish final numbers before the continuation completion report is recorded.

## Copy supported now

We redesigned Bobbin's evaluation to separate search-tool access from automatic
context injection. The offline layer measures retrieval; the paired agent pilot
measures task success. Our previous v1 aggregate cannot establish an injection
benefit and is now explicitly historical. The current work focuses on validating
fixtures, preserving interrupted-run evidence and reporting what was actually
measured before drawing conclusions.

If interim coverage is mentioned, use the exact qualifier: **partial snapshot,
25 recorded L1 cells (nine original, 16 continuation)**, not “completed pilot.”
Six tasks have four result files, but only five have four valid cells. The
continuation was running at the snapshot; these counts are not a live progress feed.
No improvement, ablation ranking, speedup or cost-saving claim is supported here.

## Fill only after the completion gate

- Link the frozen completion report and state the remaining task/repository coverage.
- Carry fixture attrition, invalid cells, incomplete tasks and source-client caveats.
- If reporting descriptive pilot outcomes, include their denominator and uncertainty.
- Separate incomplete L0 results from L1 completion; a full study remains a separate decision.
- Update this source, the book and the paper from the same reviewed report.
