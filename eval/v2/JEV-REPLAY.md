# J1–J3 offline replay contract (exploratory, not a registered campaign)

`runner.jev_replay.replay` evaluates a frozen candidate capture without changing
`l0`, `ALL_ARMS`, or the production injection hook. No service calls happen at
import time. The caller supplies `ask=client.ask` from a reviewed Jev client,
configured with the same explicit model revision passed to replay. Tests supply
a mock and consume no inference budget.

This is an evaluation seam, not a completed J1–J7 implementation. Candidate
capture in the production pipeline, neighbor-window trimming, the
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

The opt-in arms are:

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

J1 uses top-order budget filling; J2 uses the proposal/recheck algorithm below. Candidate text may
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
5. Freeze the exploratory J2 proposal/recheck contract below before collecting
   model judgments. Neither synthetic optimizer tests nor model density scores
   are measured gains; selection-dependent novelty precludes a global-optimum claim.

Focused verification: `PYTHONPATH=eval python -m pytest
 eval/tests/test_jev_replay.py eval/tests/test_l0.py`. These tests run no model,
indexer, agent, or benchmark campaign.

## J2 proposal/recheck algorithm

`jev-j2` first batches relevance and density questions for all candidates. Density
asks whether a uniformly sampled source line bears on the prompt; its noul is an
inferred fraction estimate, not measured ground truth. Drop candidates whose
estimated density is below 0.5 and those longer than the remaining line budget.
Do not truncate them to a fabricated window. Neighbor-window capture is future work.

For each round, one batched novelty query asks which remaining chunks add useful
information beyond the already selected chunks and supplied `prior_context`.
Utilities are relevance × density × novelty; novelty below 0.5 means utility zero.
Solve the current 0/1 knapsack exactly, accept the first member of its solution
in frozen candidate order, then re-evaluate novelty against the new selection.
Remove exact overlapping source ranges regardless of the model answer. Disjoint
ranges in a file remain eligible. Stop when no positive-utility chunk fits.

This is a bounded sequential approximation to semantic packing, **not** a global
optimum of the changing novelty objective. Each fixed-score subproblem is exact,
verified against exhaustive subsets across 150 deterministic generated examples.
The maximum is one relevance/density batch plus 30 novelty batches. Unlike J1,
J2 does not claim one-call cost or the same latency. Record all usage and calls.

`jev-j1-j2-j3` adds the prompt-only gate and orders candidates by the J1 blend
before the same J2 procedure. A failed later batch discards the partial selection
and returns the whole baseline as degraded fallback. No incomplete result is
scored as a successful arm. J2 supports budgets up to 600 lines; the entire
selected+remaining+prior-context state must remain within the character bound.

## Executable replay without network access

From the evaluation directory:

```sh
python -m runner.jev_replay capture.json --responses responses.json --out result.json
```

The capture is a JSON object containing the `replay` keyword arguments, with
candidate dictionaries and an `ArmScore` baseline dictionary. Responses are an
ordered list of `{state, questions, response}` records. Each saved state/question
pair must exactly match the request generated during replay. The CLI never
imports a provider, reads an API key, or makes a model call. It refuses to overwrite
an existing output. Exit 0 means an evaluated/baseline-floor result; 1 means the
arm errored/degraded and was recorded; 2 means invalid input or output refusal.
Retain full audit records; these nested JSON artifacts are distinct from ordinary
L0 score-only JSONL. Do not feed them directly into `l0-report` and silently drop
availability information.

Local assembly diagnostics are available through `context --capture-assembly`;
see [ASSEMBLY-CAPTURE.md](ASSEMBLY-CAPTURE.md). This freezes the packing inputs and
assembly baseline, but does not supply the complete hook capture required above.
