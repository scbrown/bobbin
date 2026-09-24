# J1/J3 offline replay contract (exploratory, not a registered campaign)

`runner.jev_replay.replay` evaluates a frozen candidate capture without changing
`l0`, `ALL_ARMS`, or the production injection hook. No service calls happen at
import time. The caller supplies `ask=client.ask` from a reviewed Jev client,
configured with the same explicit model revision passed to replay. Tests supply
a mock and consume no inference budget.

This is an evaluation seam, not a completed J1–J7 implementation. Candidate
capture in the production pipeline, J2 packing/trimming/semantic dedup, the
human-labelled calibration set, campaign execution and a deployment decision
remain outstanding. A passing fixture test is not evidence of a density win.

## Inputs and score meaning

Supply at most 30 `Candidate(id, path, start_line, text, fused_score)` records,
in the original adjusted-fused ranking **before** line-budget packing. The
capture must name its binary SHA256, harness and checkout commits, and stage
`post-adjustment-pre-packing`. These are caller attestations; this seam cannot
prove the exporter read that stage. Do not feed post-budget hook output and call
it a top-30 rerank. A future exporter must freeze the prompt, candidates, existing
production gate result, and baseline output together, before looking at gold.

`baseline` is the actual production `ArmScore`, not a reconstruction from the
candidate pool. It supplies the budget and whether the production gate injected.
`gold` uses the existing L0 pre-image scorer and is never included in model state
or questions. Only scoring reads it. The audit hash covers every selection input,
including baseline and capture provenance. Store the gold/scorer version alongside
the returned record when running an experiment.

The three opt-in arms are:

- `jev-j1`: one batch of relevance nouls, one per candidate. Max-normalize fused
  scores within the capture, blend `(1-alpha)*fused + alpha*relevance`, stable-sort,
  then fill the line budget. Defaults are alpha 0.5 and budget from the baseline.
  Max-normalization is an explicit experimental choice, not a calibrated optimum.
- `jev-j3`: ask only the prompt whether repository context is needed, threshold
  0.5. A positive answer preserves the actual baseline unchanged, including token
  accounting. A negative answer is a deliberate skip, scored with zero recall.
- `jev-j1-j3`: prompt-only gate first; only on a positive answer make the one
  candidate batch. This prevents candidate code from biasing the prompt decision
  and avoids spending on relevance after a negative gate.

J1 currently uses top-order budget filling, not J2 knapsack. Candidate text may
include neighboring windows only when the capture records their true source
ranges. No model-generated text is injected. Exact line ranges let the existing
L0 scorer measure density and recall. Its character/4 token estimate is retained;
this is **not** a tokenizer measurement or a count of useful facts.

## Failure and inference contract

Missing/malformed/nonfinite probabilities, wrong question IDs, wrong answer
kinds, changed model revision, and client exceptions return `status=degraded`.
The Jev score has `outcome=error` and no quality metrics; `fallback` preserves the
baseline. Count degraded rows separately in the denominator. Never count their
baseline scores as Jev successes. Error text records exception type only, since
transport exceptions may contain secrets. Client wrappers must expose actual
response model identity; a wrapper that substitutes the requested revision for a
missing response revision cannot establish pinning.

A baseline skip or error is preserved without a model call. J3 cannot override
the production similarity floor. Invalid captures or states over 60,000 JSON
characters refuse before any call; they are not silently truncated. Every audit
is `sourceKind=inferred`, `plane=quarantine`, and retains request/response evidence.
There is no graph promotion or live hook integration.

## Gates before a real run or hook change

1. Export and verify pre-packing candidate provenance; pin model revision, client,
   requests, capture bytes, binary and harness. Freeze the protocol before asking
   the model or scoring variants. Do not tune alpha/gate on evaluation outcomes.
2. Add independent human-labelled examples (~30 per slot) and a negative prompt
   set: the bug-fix tasks alone cannot measure J3 false-positive suppression.
3. Amend the preregistration: Jev-assisted replay is model-assisted and potentially
   contaminated, unlike deterministic L0. Keep existing baseline outputs intact.
   Report availability, all error counts, paired denominators, API usage/cost,
   density and recall; no complete-case-only claim that hides degradation.
4. Compare at equal budgets on the same captures; require density improvement at
   equal recall, with predeclared uncertainty and multiplicity handling, before
   any hook integration. No such result is claimed by this module.
5. J2 needs its own registered selection/novelty/trim contract. Novelty depends on
   already selected chunks, so a fixed-score 0/1 knapsack cannot honestly claim
   a globally optimal semantic packing. Document and test the chosen bounded
   approximation rather than calling a greedy sorter a knapsack.

Focused verification: `PYTHONPATH=eval python -m pytest
 eval/tests/test_jev_replay.py eval/tests/test_l0.py`. These tests run no model,
indexer, agent, or benchmark campaign.
