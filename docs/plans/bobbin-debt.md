# Bobbin tech debt — measured

> **Status (2026-08-22):** third pass, re-measured on
> `claude/quipu-bobbin-camayoc-completion-dn3i7z`. What changed since the
> second pass: §1's ten file-size errors are gone (gate now 0 errors /
> 11 warnings — see the update in §1); every bead §4 called un-closeable and
> every bead §6 called open is now **closed**, because `scripts/beads-jsonl.py`
> made the JSONL tracker writable (the very fix §4 asked for); §5's defect is
> now filed as **bobbin-daa**. Section-level notes below carry the details;
> the 2026-08-17 text is kept as the record it is.
>
> **Status (2026-08-17):** second pass. Every figure here was measured on
> `claude/neural-amplifier-progress-sg3ojv` at the date shown, not carried over
> from a bead. Where a bead and a measurement disagree, the measurement is
> recorded with the command that produced it so the next reader can re-run it
> rather than re-trust it.

`CLAUDE.md` names this file as a ranger responsibility and it did not exist.
This is the tracked-debt half; `bobbin-roadmap.md` (strategic direction)
exists as of 2026-08-21 and carries the sprint execution status.

## 1. The file-size ratchet is red — and `bobbin-aoz` had it right

> **Update (2026-08-22): the ratchet is green.** Re-running the gate the way
> this section insists it be measured:
>
> ```console
> $ scripts/check-file-size.sh --all
> File size check: 0 error(s), 11 warning(s)
> ```
>
> All ten errors are cleared by real splits, not allowlisting, and
> `bobbin-aoz` is **closed**: `src/cli/bead.rs` (the 1,179-line worst case)
> is now the `src/cli/bead/` module, the oversized handlers became
> subdirectories (`admin/`, `analysis/`, `archive/`), `sqlite/mod.rs` and
> `cross_repo` were split, and `src/index/beads.rs` dropped from a 518-line
> error to a 416-line warning. The residual debt this section named — a
> grandfathered allowlist with no retirement path — **still stands**: the
> allowlist still holds 34 entries and `src/cli/hook.rs` has grown to 9,500
> lines inside it; the gate still has no mechanism for shrinking it.

> **Correction (2026-08-17).** An earlier revision of this section claimed the
> bead understated the problem by 4× — "bobbin-aoz says 10 files over the
> 500-line limit. There are 42." **That correction was wrong and the bead was
> right.** The retraction is left in place rather than deleted because the
> incorrect figure was carried into `bobbin-aoz` as a scope-correction note and
> would otherwise have re-scoped the bead by an order of magnitude in the wrong
> direction.

The 42 came from a raw `find` that ignored both of the gate's own exemptions.
`scripts/check-file-size.sh` skips `*tests.rs` / `*_test.rs`, and skips the 34
entries in `scripts/large-file-allowlist.txt`. Running the gate rather than
re-deriving it:

```console
$ scripts/check-file-size.sh --all
ERROR: src/cli/bead.rs has 1179 lines (limit: 500)
ERROR: src/cli/mod.rs has 516 lines (limit: 500)
ERROR: src/cli/ontology.rs has 663 lines (limit: 500)
ERROR: src/http/handlers/admin.rs has 575 lines (limit: 500)
ERROR: src/http/handlers/analysis.rs has 502 lines (limit: 500)
ERROR: src/http/handlers/archive.rs has 846 lines (limit: 500)
ERROR: src/http/handlers/webhook.rs has 516 lines (limit: 500)
ERROR: src/index/beads.rs has 518 lines (limit: 500)
ERROR: src/index/cross_repo.rs has 620 lines (limit: 500)
ERROR: src/storage/sqlite/mod.rs has 542 lines (limit: 500)

File size check: 10 error(s), 6 warning(s)
```

**Ten errors. Exactly what the bead filed**, `src/cli/bead.rs` at 1,179 lines
as the worst case, exactly as the bead named it. The big files the earlier
revision listed — `hook.rs` at 8,050, `lance.rs` at 5,038 — are all
*allowlisted*, deliberately, as grandfathered entries.

The lesson is the one the section was originally trying to make, turned around:
**measure the gate by running the gate.** A re-derivation that drops the
exemptions is not a stricter measurement, it is a different one.

**The bead's remedy is therefore correctly scoped as filed** — split the ten or
allowlist them with rationale — and needs no re-scoping before dispatch. What
it does need is a decision on the allowlist itself, which is the real debt here:
34 grandfathered entries including an 8,050-line file means the gate protects
new code and has no mechanism for retiring old. A ratchet with no retirement
path is a ratchet that only ever loosens.

**Not actioned here.** Splitting these is the "large code change" the ranger
charter reserves for polecats.

## 2. Fixed this pass

### bobbin-lpp — intent doc-demotion inverted on the local path

P1, confirmed exactly as filed and fixed this pass.

`doc_demotion` is a score **multiplier** — 1.0 leaves doc scores alone, 0.0
suppresses them — so *lower* means *stronger* demotion. The remote hook path
knew this and worked in effect space; the local path raw-multiplied:

```text
base 0.5, BugFix factor 1.5
  remote:  effect = (1-0.5)*1.5 = 0.75 -> 1-0.75 = 0.25   MORE demotion
  local:            0.5  * 1.5         =        0.75      LESS demotion
```

`src/search/intent.rs`'s own tests assert the contract the local path was
breaking — `assert!(adj.doc_demotion_factor > 1.0); // More demotion = less
docs`. So every intent was inverted on the local path: BugFix and Operational
surfaced *more* docs, Architecture surfaced *fewer*, exactly backwards.

**Fixed as a shared function, not a corrected line.** Both paths now call
`search::intent::apply_doc_demotion_factor`, because the defect was two copies
of one calculation disagreeing — patching the local copy would have left the
next author free to write a third. The `floor` differs by caller (local keeps
0.01 so a doc score is never multiplied to exactly zero; remote allows 0.0) and
is a parameter rather than a second implementation.

Seven tests, including `test_every_shipped_intent_moves_demotion_the_direction_its_comment_claims`,
which walks the whole intent table and asserts the arithmetic delivers what each
entry's comment says. That is the check that would have caught this at the
source. Full suite green: 923 passed, 0 failed; clippy warning count unchanged
at 207.

**One self-inflicted problem, caught and undone.** The new tests pushed
`src/search/intent.rs` from 428 to 530 lines, over the 500 error limit — an
eleventh failure on the very gate §1 describes as trained-to-be-ignored. Adding
an allowlist entry would have been the easy exit and exactly the wrong one. The
test module moved to `src/search/intent_tests.rs`, which
`scripts/check-file-size.sh` exempts by design. `intent.rs` is now 305 lines,
under the 400-line *warning* threshold it had already crossed before this
session, so the gate ends the pass one warning better than it started: 10
errors and 6 warnings, from 10 and 7.

### bobbin-au4 — complementary expansion kept non-adjacent duplicates

`dedup_by` removes only **consecutive** equal entries, and the sort above it was
by score rather than by path, so two entries for the same file were adjacent
only by coincidence. A file coupled to several already-seen files survived as
several entries and could consume most of the 5 slots — crowding out the
distinct suggestions the truncation exists to allocate.

Extracted to `dedupe_complementary` (max score per path, sorted, truncated) so
it is testable at all; the old form was buried mid-function. Ties break by path,
because `HashMap` iteration order varies run to run and would otherwise make the
injected context differ between identical invocations.

Five tests, the load-bearing one being that five slots hold five *distinct*
files — pre-fix that case returned one file four times and lost four real
suggestions. 928 passed, 0 failed.

I had previously deferred this as polecat work on the grounds that it changes
what gets injected. That reasoning was inconsistent: `lpp` changes ranking and I
fixed it. The real distinction is `zhx`, which changes session *identity* and so
needs a migration decision — au4 has no such concern and is simply a bug.

### bobbin-10d — stale grid-count comment (`src/cli/calibrate.rs`)

Verified against the code rather than accepted from the bead, and the bead was
right in every figure. `run_full_sweep`'s doc comment claimed 240 configs and
19,200 probes; it stopped counting after the first five grid dimensions and
omitted budgets, search limits and bridge mode/factor. Real geometry at default
args:

```text
5 sw × 3 dd × 1 k × 4 hl × 4 rw × 3 budgets × 4 limits × 8 bridge = 23,040
  × 4 coupling depths                                             = 92,160
  × 20 commits                                                    = 1,843,200
```

Off by 96×. The bridge term is 8 and not 12 because `Off` and `Inject` ignore
the boost factor.

Fixed, and pinned: `test_full_sweep_grid_geometry` and
`test_pinned_dimensions_shrink_the_grid` assert the arithmetic, so the comment
cannot drift again without a test failing. The bead offered "update the comment
or derive it from the constants" — a test is the third option, and the better
one, because deriving it would have put the same uncheckable arithmetic in a
different place.

## 3. Specified, then executed

> **Update (2026-08-17):** `bobbin-zhx` is **fixed**, and the migration concern
> that held it back was mistaken. The original analysis is kept below because
> its *fix* was correct in every detail; only its risk assessment was wrong.
>
> **The claim was:** "it changes session identity, so anything keyed on a
> session id — the session ledger's progressive-reduction state in particular —
> sees a discontinuity on upgrade."
>
> **The `SessionLedger` is not keyed on this fingerprint.** It is keyed on
> Claude Code's own session id (`SessionLedger::load(&repo_root, &input.session_id)`,
> stored under `.bobbin/session/<cc_session_id>/`). `compute_session_id`'s
> output is `dedup_session_id`, and its only consumer is the binary-dedup
> fallback that runs *when reducing is disabled*:
>
> ```rust
> } else if dedup_enabled && !reducing_enabled {
>     let s = load_hook_state(&repo_root);
>     if s.last_session_id == dedup_session_id && !dedup_session_id.is_empty() {
> ```
>
> So the real upgrade impact is: a user who has turned `reducing_enabled` off
> gets **one** non-skip on the first prompt after upgrading, because the stored
> fingerprint predates the formula change. Then it self-heals. `reducing_enabled`
> defaults to `true`, so the default configuration never reaches this path at
> all. That is not a migration, and it needed no ledger test.
>
> Applied with seven tests in `src/cli/hook_session_id_tests.rs`, including the
> two that pin the subtle half — that reordering scores without changing the
> selected set must not move the fingerprint, and that ties break by key rather
> than by bundle iteration order. The tests live in their own file because
> `hook.rs` is the largest file in the tree; growing it by another hundred lines
> of tests is the drift the allowlist exists to bound. Suite: 939 passed, 0
> failed.

### bobbin-zhx — topic fingerprint truncates alphabetically

Confirmed exactly as filed, `src/cli/hook.rs:2273-2293`:

```rust
keys.sort();        // lexicographic, by "path:start:end"
keys.truncate(10);
```

`c.score` is in scope in the comprehension directly above and is discarded.
Which chunks enter the session fingerprint therefore depends on path
lexicography — a file under `src/a…` displaces a far more relevant one under
`src/z…`. The doc comment describes the mechanism accurately ("sorts
alphabetically, takes top 10"), so this is a design defect rather than a
code-comment mismatch, which is why it needs a decision and not just a patch.

**The fix, precisely:**

```rust
keys.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
keys.truncate(10);
keys.sort();        // re-sort AFTER truncating, for hash stability
```

The final re-sort is the subtle part and the reason this is written out: the
hash must not depend on score ordering, only on the *set* of chunks selected.
Sorting by score and hashing in that order would make the fingerprint unstable
whenever two chunks' scores changed relative to each other without the selected
set changing.

**Why not applied here.** It changes session identity: every existing
fingerprint changes value, so anything keyed on a session id — the session
ledger's progressive-reduction state in particular — sees a discontinuity on
upgrade. That needs a deliberate decision about migration and a test over the
ledger, which is polecat work with a spec, not a four-line edit by the ranger.
`f32` also has no total order, hence the explicit `partial_cmp` fallback rather
than `sort_by_key`.

## 4. Verified-implemented but not closeable

> **Update (2026-08-22): closeable, and closed.** The fix-at-the-source this
> section asked for exists as `scripts/beads-jsonl.py` — a guarded writer that
> treats `.beads/issues.jsonl` as the tracker itself (this repo has no Dolt
> store to derive it from) and refuses any information-losing write. All seven
> yupana-tagged beads below are now closed through it, as are `bobbin-lpp`,
> `-au4`, `-10d` (§2) and `-zhx` (§3). The shadow-state concern is retired.

Seven yupana-tagged beads were checked against the code this session and are
already implemented: `bobbin-ha6`, `bobbin-4xj`, `bobbin-052`, `bobbin-bnq`,
`bobbin-9k3`, `bobbin-fjh`, `bobbin-tvn`. Commit `0245984` closed three of them.

**They cannot be closed through `bd` from here.** This repository has
`.beads/issues.jsonl` — the passive export — and no Dolt database, and no
`refs/dolt/data` on the remote. Running `bd init` would mint a divergent tracker
identity rather than adopt the existing one, and hand-editing the JSONL is the
anti-pattern its own README names. So the record lives here and in commit
messages until someone with the Dolt remote closes them.

This is worth fixing at the source: a repo whose tracked JSONL cannot be written
back to accumulates exactly this kind of shadow state.

## 5. Found this pass, unfiled — the eval harness does not record its own model

> **Update (2026-08-22): filed as `bobbin-daa`** (P2, pitch).
> `scripts/beads-jsonl.py` gained the `create` subcommand this section was
> waiting on. The defect itself is unchanged — the runner still does not
> persist the serving model from the agent's usage record, and the scorer
> still pools across it (`scripts/paper_census.py::serving_model` is the
> post-hoc reader, not the fix).

**No bead existed for this at the time of writing**; `scripts/beads-jsonl.py`
had no `create` subcommand, so it was recorded here until one could be filed.
Severity: this is the defect that cost the paper its strongest result.

**What it is.** The eval runner does not reliably record which model served a
run. Of the 85 completed runs in `eval/results/runs/`, 29 carry a confirmed
serving model in `agent_result.model_usage`, 11 confirm a *different* model
(`claude-opus-4-6`) than the study intended, 20 have a manifest that declares
`claude-sonnet-4-5-20250929` but no usage record to confirm it, and 25 carry no
model information of any kind. **45 of 85 runs cannot be attributed to a model
from their own artifact.**

**What it cost.** The paper's ablation baseline turned out to be model-mixed
while every ablation arm was uniformly Sonnet 4.5. Re-basing on a model-matched
baseline moves all six removal effects, flips three signs, and drops the one
nominally significant arm from p = 0.030 to p = 0.191. See
`docs/paper-context-injection.md` §5.1.1 and `docs/plans/paper-statistics.md`
§4b. None of it is recoverable from the existing runs — an unattributed run
stays unattributed.

**The fix, in two parts.** Both are small and neither is optional before the
next study:

1. **Record it.** Persist the serving model into every run artifact at write
   time, from the agent's own usage record rather than from the config that
   requested it. A manifest field that declares intent is what failed here.
2. **Refuse to compare across it.** The scorer should treat serving model as
   part of a run's identity and decline to aggregate arms that do not match,
   rather than silently pooling them. An unattributed run should be excluded
   from cross-arm comparison, not assumed to match.

Point 2 is the load-bearing one. Point 1 alone would have surfaced the problem
in the artifacts, which is where it was eventually found — nine months late,
by a recount undertaken for an unrelated reason.

## 6. Open and unassessed

> **Update (2026-08-22): all three are closed.** `bobbin-aa0` shipped as
> `InjectionTurn` in `src/cli/hook.rs` (failure and post-tool-use injections
> now carry IDs and meet the ledger). `bobbin-bbe` shipped reshaped as
> `bobbin bead similar` (the `bd create` trigger point the bead assumed does
> not exist here). `bobbin-di7` was specified after all — delivered as the
> quipu integration epic, finished by the pin bump to 0.3.23 with the SHACL
> gate compiled in (`bobbin-c58`, commit `4efb900`). The assessments below
> are kept as the record of what they looked like when open.

`bobbin-aa0`, `bobbin-di7`, `bobbin-bbe`.

Read but not actioned:

- **`bobbin-aa0`** (P2) — failure and post-tool-use injections carry no
  injection ID and never touch the `SessionLedger`, so they cannot be rated via
  feedback and can re-inject chunks the prompt path already delivered. This is
  a real gap and genuinely large: it touches every injecting event class.
- **`bobbin-bbe`** (P3) — duplicate work-item advisory at bead creation. Same
  shape as camayoc-b6h, which was implemented this session with a deterministic
  matcher; bobbin already embeds the beads corpus (`src/index/beads.rs`), so the
  scorer here is *not* blocked the way camayoc's was.
- **`bobbin-di7`** (P2, "Quipu Integration") — title only, no actionable
  description. Needs specification before it can be dispatched at all.
