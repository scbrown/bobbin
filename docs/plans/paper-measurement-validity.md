# Which paper measurements survive — bobbin-53

> **Status (2026-08-17):** settled. Every claim below was established against
> the code and the eval harness at the commit named, not inferred from the
> bead. The load-bearing findings are pinned by tests
> (`src/search/eval_prompt_intent_tests.rs`) so a future reader can re-run them
> rather than re-trust this document.

`bobbin-53` asked a narrow question: the context-injection paper is dated
2026-03-05, two bugs in mechanisms it ablates were fixed on 2026-08-17, so
which arms of the 66-run and 85-run studies are still valid?

**Answer: both fixes are no-ops for these measurements, for reasons that are
structural rather than lucky. No arm needs re-running on their account.**

That is the good news, and it is narrow. The investigation surfaced a
*different* threat to the same tables which is considerably worse, and which
no bead had recorded. It is §3.

---

## 1. `cee3cdc` (bobbin-au4) — complementary expansion — **cannot have run**

The fix corrected a dedup in the complementary-expansion path. That path is
unreachable under the eval harness, so no eval number can contain the bug.

The branch is guarded twice (`src/cli/hook.rs`, step 9):

```rust
if bundle.files.is_empty() || new_chunks == 0 {
    if reducing_enabled && reduced_count > 0 {
        // 9a. Complementary expansion
```

`reduced_count` is the number of chunks filtered out for having been injected
*earlier in the same session*. It can only exceed zero on a **second or later**
`UserPromptSubmit`, once the `SessionLedger` holds entries from a previous turn.

The eval harness runs each task as a single-shot headless invocation —
`eval/runner/agent_runner.py`:

```python
cmd = [claude, "-p", prompt, "--output-format", "stream-json", ...]
```

`claude -p` delivers exactly one `UserPromptSubmit`. On that one turn the
ledger is empty, `reduced_count == 0`, and the guard fails. Complementary
expansion never executed in any of the 66 or 85 runs.

**Note this is not the same mechanism as the `coupling_depth` arm**, which the
bead correctly flagged as needing checking rather than assuming. Confirmed
distinct: `coupling_depth` is an *indexing* parameter (`config.git.coupling_depth`,
consumed at `src/cli/index.rs:709`) controlling how many commits are mined into
the co-change table, and the ablation disables coupling by starving that table.
Complementary expansion is a query-time fallback that reads the same table via
`get_coupling(seen_file, 5)` with a hardcoded limit. They share a data source
and nothing else. **The −0.247 coupling arm is unaffected by `cee3cdc`.**

## 2. `7d99f03` (bobbin-lpp) — doc demotion — **the code did not exist yet**

Unlike au4, this one is on the eval's live path *today*. `eval/settings-with-bobbin.json`
registers `bobbin hook inject-context`, which is the **local** hook path — the
exact path whose doc-demotion arithmetic was inverted.

But the intent machinery the bug lived in was not running when the study was
measured, and the run artifacts prove it rather than merely suggesting it.

**The proof is an early return.** Today, `inject-context` refuses to inject at
all for `Operational` intent, before any doc-demotion arithmetic is reached:

```rust
if intent == crate::search::intent::QueryIntent::Operational {
    eprintln!("bobbin: skipped (operational intent: {:?})", intent);
    return Ok(());
}
```

Under today's classifier, three of the five Ruff tasks classify as
`Operational` — pinned in `src/search/eval_prompt_intent_tests.rs`:

| Task | Intent today | `doc_demotion_factor` |
|------|--------------|----------------------|
| ruff-003, ruff-004, ruff-005 | `Operational` | 2.0 (and injection skipped) |
| the other ten | `Navigation` | 1.0 |

If that code had been live in February 2026, those three tasks' `with-bobbin`
runs would have injected **nothing**. They injected:

```console
$ # eval/results/runs/*/ruff-00{3,4,5}_with-bobbin_0_metrics.jsonl
ruff-003  2026-02-27T04:16:08  chunks_returned: 20
ruff-004  2026-02-27T04:24:35  chunks_returned: 20
ruff-005  2026-02-27T04:31:55  chunks_returned: 19
```

Twenty, twenty and nineteen chunks. The `Operational` skip was therefore not in
the binary that produced these runs, and neither was the intent-adjustment path
that `bobbin-lpp` corrected — they are the same code, added together. The fix
cannot have altered a measurement taken before the code being fixed existed.

(The repository's git history begins 2026-07-24 and does not reach the
measurement dates, so this had to be established from run artifacts rather than
from `git log`. That is itself worth recording: the code that produced the
paper's numbers is not in this repo's history.)

**Two consequences beyond bobbin-53.** Both belong in the paper.

1. **The study did not measure intent-adaptive behaviour**, because the feature
   did not exist. Any present-tense claim about default configuration in the
   paper describes a system materially different from the one measured.
2. **If the study were re-run today it still would not measure it.** Intent is
   classified on the **last 500 characters** of the prompt. All 40 task prompts
   in `eval/tasks/` exceed 500 characters, and `_build_prompt`
   (`eval/runner/cli.py`) appends a **416-character** instruction block that is
   byte-identical across every task and every ablation arm:

   ```text
   This project has bobbin installed (a semantic code search engine). Before
   exploring manually, use bobbin to find relevant code:
   - `bobbin search "<key terms from the task>"` — semantic + keyword search
   - `bobbin related <file>` — find test files and co-changing dependencies
   - `bobbin refs <SymbolName>` — trace definitions and usages
   Start with bobbin search to orient yourself, then read the files it identifies.
   ```

   So the classification window is ~84 characters of task text followed by 416
   characters of boilerplate dense in navigation signals (`find`, `files`,
   `definitions`, `read the`). `test_the_shared_instruction_block_alone_drives_the_classification`
   asserts the block classifies as `Navigation` on its own and that its verdict
   dominates the corpus. **The harness would be measuring its own prompt
   template.** Fix the harness — classify on task text — before running any
   intent-sensitive experiment.

## 2a. The gate arm injected nothing, and that is the study's most useful number

Found while verifying the above, and it is the single most consequential thing
in this document.

`gate_threshold=1.0` is listed in §4.4 as "disable quality gating". Per the
paper's own §3.7, `gate_threshold=1.0` means **never inject**. Those are not the
same operation, and the run artifacts confirm the latter: all three runs record
an `inject-context` invocation with **no chunks returned**, while the other arms
record 17-20.

```console
gate_threshold=1.0   injections=1  chunks=None   (x3 runs)
coupling_depth=0     injections=1  chunks=17     (x3 runs)
blame_bridging=false injections=1  chunks=17     (x3 runs)
```

So this arm is not an ablation of gating. It is a **third baseline**: the agent
gets the bobbin prompt block and the bobbin CLI, uses them (3-5 `bobbin search`
calls per run, plus `related`/`refs`), and receives no automatic injection.

That accidentally supplies the control the study otherwise lacks, and it
decomposes the headline effect:

| Contrast | What differs | Δ F1 | p |
|----------|--------------|-----:|---:|
| no-bobbin → `gate_threshold=1.0` | tool available + prompted, no injection | **+0.287** | 0.288 |
| `gate_threshold=1.0` → with-bobbin | automatic injection added | **+0.025** | 0.922 |
| no-bobbin → with-bobbin | both | +0.312 | 0.055 |

**92% of the measured benefit is present before any automatic injection
happens.** On these point estimates, telling the agent a search tool exists and
letting it call the tool itself accounts for nearly all of the effect; the
automatic injection this paper is about adds 0.025.

Every caveat in `paper-statistics.md` applies — N=3/5/7, nothing significant,
the 0.287 interval is enormous. It is not a result. It is a **confound the
design cannot separate**, and it must be stated in the paper, because the
paper's central claim is about automatic injection and its own data cannot
distinguish that from tool availability. The fix is a fourth arm — bobbin
installed and prompted but the hook removed — run at proper N.

## 3. The larger problem the bead did not ask about: the Flask tasks

`eval/tasks/_quarantined/README`:

```text
flask-001 through flask-005: 0% test pass rate on BOTH with-bobbin and
no-bobbin approaches (47 runs, Feb 10-11 2026). Root cause appears to be
setup_command or test_command issues, not bobbin. Re-enable after fixing
the flask task definitions.

Moved here 2026-02-15 as part of aegis-rdpt investigation.
```

The tasks were quarantined on **2026-02-15**. The paper is dated **2026-03-05**
— eighteen days later — and still reports all five in Table 2, still counts
them among "13 tasks", and still builds §5.4's entire calibration sweep on
"4 Flask tasks". §6.2 then concludes:

> **Flask tasks show minimal benefit.** Five Flask tasks showed mixed results
> (-0.100 to +0.063 F1 delta), possibly because Flask's well-organized codebase
> and clear naming conventions make agent exploration already effective.

That explanation is unsupported. The tasks had a **0% test pass rate on both
arms** from broken `setup_command`/`test_command` definitions. The flat F1
deltas are what a broken harness produces, and the paper reads them as a
property of Flask's code quality.

This matters more than either bug fix:

- Flask is **5 of 13 tasks** — the largest single block in the 66-run aggregate.
- The **entire calibration sweep (§5.4, Table 6)** rests on Flask tasks. Its
  conclusion — that `sw=0.90` beats the 0.70 default — is drawn from tasks
  known to be broken, and is currently the paper's only evidence for a
  configuration recommendation.
- §6.2's Flask paragraph is an affirmative claim built on the artifact.

**Required before submission**, in priority order:

1. Drop Table 6 and §5.4 entirely, or re-run the sweep on unquarantined tasks.
   Do not report a calibration recommendation from quarantined tasks.
2. Recompute Table 1 with and without the Flask block and report both, or
   exclude Flask and restate N. The current "66 runs across 13 tasks" is not
   the study that was run.
3. Delete the §6.2 Flask explanation. Replace with the quarantine fact.

## 4. What else does not reproduce

Smaller, but they are reproduction blockers and cheap to state:

- **The task suite in the repo is not the task suite in the paper.** `eval/tasks/`
  now holds 40 tasks across cargo, django, go, nushell, pandas, polars, ruff and
  typst. The paper describes 13 across Ruff, Flask, Cargo and Polars. Anyone
  following Appendix B gets a different study.
- **Appendix B's `run-baseline-study.sh` comment says "4 tasks x 8 conditions x
  3 attempts"** = 96, while the appendix text says 108 runs and the abstract
  says 85. Three numbers for one study.

## 5. Verdict per arm

| Arm | `cee3cdc` | `7d99f03` | Verdict |
|-----|-----------|-----------|---------|
| no-bobbin | n/a (no hook) | n/a (no hook) | **Valid** |
| with-bobbin | unreachable | factor 1.0 | **Valid** |
| `semantic_weight=0.0` | unreachable | factor 1.0 | **Valid** |
| `coupling_depth=0` | unreachable, and distinct mechanism | factor 1.0 | **Valid** |
| `recency_weight=0.0` | unreachable | factor 1.0 | **Valid** |
| `doc_demotion=0.0` | unreachable | factor 1.0 | **Valid** |
| `gate_threshold=1.0` | unreachable | factor 1.0 | **Valid** |
| `blame_bridging=false` | unreachable | factor 1.0 | **Valid** |

No re-run is required on account of the two bug fixes, and no pinned-commit
caveat is required either — the mechanisms did not participate.

**Valid here means "not corrupted by the two fixes bobbin-53 named". It does
not mean the numbers support the paper's claims.** They largely do not: see
`docs/plans/paper-statistics.md` for the significance and power analysis, where
no ablation arm survives multiple-comparison correction. The two questions are
independent and both had to be answered.
