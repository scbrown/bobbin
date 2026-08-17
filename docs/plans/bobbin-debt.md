# Bobbin tech debt — measured

> **Status (2026-08-17):** first pass. Every figure here was measured on
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

`bobbin-lpp` (P1, intent doc-demotion factor has opposite semantics local vs
remote), `bobbin-aa0`, `bobbin-au4`, `bobbin-di7`, `bobbin-bbe`. `lpp` is the
one to look at first — a P1 with two code paths disagreeing about the direction
of a ranking factor is a correctness bug in the ranker, and neither path is
obviously the intended one from the bead text alone.
