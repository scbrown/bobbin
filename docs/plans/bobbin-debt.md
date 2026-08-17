# Bobbin tech debt — measured

> **Status (2026-08-17):** second pass. Every figure here was measured on
> `claude/neural-amplifier-progress-sg3ojv` at the date shown, not carried over
> from a bead. Where a bead and a measurement disagree, the measurement is
> recorded with the command that produced it so the next reader can re-run it
> rather than re-trust it.

`CLAUDE.md` names this file as a ranger responsibility and it did not exist.
This is the tracked-debt half; `bobbin-roadmap.md` (strategic direction) is
still absent.

## 1. The file-size ratchet is red, and much worse than filed

**bobbin-aoz says 10 files over the 500-line limit. There are 42.**

```console
$ find src -name '*.rs' -exec wc -l {} + | awk '$1>500 && $2!="total"' | wc -l
42
```

The ten the bead names are real, but they are not the large ones — it lists
`src/cli/bead.rs` at 1,179 lines as the worst case. The actual top of the list:

| Lines | File |
|------:|------|
| 8,050 | `src/cli/hook.rs` |
| 5,038 | `src/storage/lance.rs` |
| 3,208 | `src/mcp/server.rs` |
| 2,608 | `src/search/context.rs` |
| 2,513 | `src/index/parser.rs` |
| 2,168 | `src/cli/bundle.rs` |
| 2,166 | `src/reactions.rs` |
| 2,013 | `src/cli/index.rs` |

`src/cli/hook.rs` alone is sixteen times the limit and nearly seven times the
largest file the bead mentions.

**Why the understatement matters more than the number.** The bead's remedy is
"split the ten files or move them deliberately to the allowlist with rationale".
Against 42 files headed by an 8,050-line one, that is not the same piece of
work, and anyone scoping from the bead would size it wrong by an order of
magnitude. The bead needs re-scoping before it is dispatched, not just
re-prioritising.

**Not actioned here.** Splitting these is exactly the "large code change" the
ranger charter reserves for polecats. The correction is the deliverable.

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

## 3. Specified, not executed

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

## 5. Open and unassessed

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
