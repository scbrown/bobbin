# Separating Retrieval Tools from Automatic Context Injection

**Bobbin evaluation v2 — paper draft v0.3, September 2026**

**Publication status: partial methods and coverage draft.** No final result,
effectiveness claim or confirmatory conclusion is reported. The continuation must
finish and its completion report must be recorded before final numerical findings
are added. The pilot will remain descriptive after completion.

## Abstract

Evaluation of coding-agent context systems can conflate access to a retrieval tool
with automatic injection of retrieved code. Bobbin evaluation v2 separates these
mechanisms through offline retrieval measurements and a paired 2×2 agent pilot.
The preregistered primary agent outcome is hidden-test task success, rather than
file overlap with a reference patch. This draft specifies the reporting structure
and audits an incomplete saved-output snapshot. Interrupted execution, fixture
attrition and invalid cells prevent treating that snapshot as a completed study.
No improvement estimate or ranking of retrieval methods is claimed.

## 1. Research questions and prior-study limits

We ask whether retrieval components affect the relevance of injected context, and
whether search-tool availability and automatic injection have separable effects
on task success. These questions require different measurements.

The [archived v0.2 paper](paper-context-injection-v0.2.md) describes exploratory v1
analyses. Its mixed baseline models, confounded tool/injection treatment, small
ablation arms and file-F1 proxy do not establish an effectiveness result. Its
point-estimate rankings and tool-attribution percentages are not carried forward
as findings. No v1 observations are pooled into v2.

## 2. Preregistered design

The [registration](../eval/v2/PREREGISTRATION.md) fixes the task set, exclusions,
line budgets, arms, seeds, outcomes, comparisons and amendment process.

L0 runs the production injection path on task prompts at parent checkouts without
an agent. Hunk recall and useful-line density at 300 lines are co-primary; file
metrics and budget sensitivity are secondary. Six removal arms and four retrieval
baselines compare with `full`. The registered analysis specifies task-level
bootstrap intervals, paired tests and Holm correction by outcome family. Retrieval
errors and empty injections require explicit coverage accounting.

L1 gives each task four conditions: neither mechanism, tool only, injection only,
and both. It fixes one model and holds prompts, limits and checkouts constant.
Task success on the hidden tests is primary; file F1, usage, time and cost are
secondary. The pilot is descriptive and cannot justify the registered full-study
hypotheses on its own. The full study requires a separate decision; L2 has no
reported results.

## 3. Provenance and recovery

The original pilot was interrupted. The [recovery contract](../eval/v2/PILOT_RECOVERY.md)
excludes every previously attempted task from the continuation, including tasks
with fixture failures and partial cells. New fixture controls require an executed
failure at the parent and an executed pass at the known fix. The continuation
preserves the original runtime settings and pins its client executable; original
client identity remains a limitation. Original and continuation source labels
must remain visible in any later descriptive pooling.

The nine stored original grader outputs were replayed without executing tests or
models before continuation launch; passed/valid/executed classifications did not
change. This checks scorer compatibility, not full environment equivalence.

## 4. Partial coverage, not final results

The [partial artifact](../eval/v2/reports/partial-20260927.json) identifies the
snapshot time and each L1 result digest. The accompanying
[book page](book/src/eval/summary.md) is the canonical coverage table.

At that snapshot there are **25 recorded L1 cells: nine original and 16 continuation**.
Six tasks have all four result files, but only five have four valid cells. One
original task is incomplete and one recorded continuation cell is invalid. These
counts are diagnostic, not a post-hoc analysis denominator. Final reporting must
show outcome-policy decisions and preserve invalid and missing observations.

The partial L0 file contains **858 score records for 26 of 40 planned tasks**,
across 11 arms and three budgets. Its outcome counts are 569 injected, 212 skipped
and 77 errors. Replay of the saved 300-line scores succeeds, but completion and
error accounting must be settled before quality comparisons are published.

No task-success rates, paired effect estimates, inferential tests or cost totals
are promoted from these partial outputs into the paper.

## 5. Planned results presentation

For L0, report eligible tasks, errors, skips, undefined-density cases and the actual
paired sample size for each registered contrast before estimates and uncertainty.
For L1, start with the planned-to-attempted-to-valid task flow, split by source.
Show task outcomes and invalid/incomplete cells separately, followed by descriptive
paired comparisons under the registered policy. Cost coverage must identify any
stream without a terminal billing record. Missing cost is unknown, not zero.

All deviations must retain dates and reasons. This draft does not amend the
registration or authorize further model calls.

## 6. Limitations and claim boundary

The retained pilot is conditioned on available discriminating fixtures, with
substantial task and repository attrition. Source-client provenance differs.
Public historical fixes may have appeared in model training; no out-of-distribution
claim follows from using held-out local checkouts. Repeated cells are not extra
independent tasks. File F1 remains a secondary proxy and cannot stand in for
successful execution. An interrupted or invalid observation is not silently a pass.

## 7. Conclusion pending

The current deliverable is a preregistered evaluation and an auditable account of
coverage. Whether either treatment improves task success remains unanswered.
Final findings are withheld until the continuation and completion report gate is
met. See the [replay and publication checklist](../eval/v2/RESULTS.md).
