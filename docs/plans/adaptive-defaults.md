# Design: Adaptive Defaults — Project-Aware Search Configuration

## Problem

Bobbin's search parameters are one-size-fits-all defaults tuned for small-to-mid
Python projects. Ablation testing reveals these defaults actively hurt search
quality on large, identifier-heavy codebases:

| Project | Size | Language | Baseline F1 (sw=0.9) | Best F1 | Best Config |
|---------|------|----------|-----------------------|---------|-------------|
| Flask | ~8k LOC | Python | 0.461 | 0.527 | no_coupling (sw=0.9) |
| Ruff | ~400k LOC | Rust | 0.194 | 0.301 | no_semantic (sw=0.0) |

Semantic search helps on small Python repos but hurts on large Rust monorepos.
The optimal `semantic_weight` flips direction entirely based on project characteristics.

## Evidence

### Cross-Project Ablation (2026-02-25)

**What flips:**
- `semantic_weight`: Flask wants 0.9, ruff wants 0.0 — **opposite ends**
- `coupling`: Helps to remove on Flask (+0.066), neutral on ruff

**What's stable:**
- `doc_demotion`: Always critical. Removing it hurts both Flask (-0.142) and ruff (-0.094)
- `recency_weight`: Neutral on both projects at current settings

**Why semantic hurts on ruff:**
- 57k chunks vs ~2k for Flask — signal-to-noise ratio collapses
- Rust has explicit, grep-friendly identifiers (`FBT003`, `PLC2701`, `flake8_boolean_trap`)
- Monorepo structure (100+ similarly-structured `crates/*/src/rules/*.rs`) confuses embeddings
- BM25 excels at exact identifier matching

### Coupling and Recency — Untested Parameter Space

Current defaults: `recency_half_life_days=30`, `coupling_depth=5000`, `recency_weight=0.3`.

Ablation showed both as neutral, but this may be because:
1. Half-life of 30 days may be too narrow for repos with slower cadences
2. `coupling_depth=5000` may be insufficient for ruff's long history
3. Eval tasks may not exercise these features (ground truth is from arbitrary commits)

**Action needed**: Ablation variants that sweep `recency_half_life_days=[7, 30, 90, 365]`
and `coupling_depth=[100, 1000, 5000, 20000]` to find signal.

## Design

### 1. Project Inspection (`bobbin inspect`)

A new command that analyzes a project and recommends configuration. Can be run
standalone or integrated into `bobbin init`.

```
$ bobbin inspect

Project Profile
  Languages:   rust (68%), python (23%), markdown (9%)
  Files:       4,940
  Chunks:      57,158 (estimated)
  Repo age:    3.2 years
  Commit rate: 12.4 commits/week
  Structure:   monorepo (47 top-level crates)

Recommended Configuration
  semantic_weight:       0.30  (large codebase, identifier-heavy languages)
  doc_demotion:          0.30  (standard)
  rrf_k:                 60.0  (standard)
  recency_half_life_days: 14.0 (active development)
  recency_weight:        0.30  (standard)
  coupling_depth:        10000 (3.2 year history)

  Apply? [Y/n]
```

**Inspection signals:**

| Signal | How to Measure | What it Affects |
|--------|---------------|-----------------|
| Chunk count (estimated) | Count files × avg chunks/file from language stats | `semantic_weight` |
| Primary language | Tree-sitter language distribution | `semantic_weight` |
| Identifier style | Sample identifiers, measure avg length + uniqueness | `semantic_weight` |
| Repo age | `git log --reverse --format=%ct \| head -1` | `coupling_depth` |
| Commit frequency | Recent commits / time window | `recency_half_life_days` |
| Monorepo structure | Count top-level dirs with independent build files | `semantic_weight` |
| Doc ratio | Markdown + config files vs source files | `doc_demotion` |

### 2. Adaptive Defaults Tiers

Rather than a continuous function, use discrete tiers that are easy to reason about:

**Tier: Small Project** (< 5k chunks, estimated < 500 files)
```toml
[search]
semantic_weight = 0.90
doc_demotion = 0.30
rrf_k = 60.0
recency_half_life_days = 30.0
recency_weight = 0.30
```

**Tier: Medium Project** (5k–30k chunks)
```toml
[search]
semantic_weight = 0.70
doc_demotion = 0.30
rrf_k = 60.0
recency_half_life_days = 30.0
recency_weight = 0.30
```

**Tier: Large Project** (30k+ chunks)
```toml
[search]
semantic_weight = 0.40
doc_demotion = 0.30
rrf_k = 60.0
recency_half_life_days = 14.0
recency_weight = 0.30
```

**Language modifier:** Identifier-heavy languages (Rust, Go, Java, C++) shift
`semantic_weight` down by 0.1–0.2 from the tier default because BM25 is more
effective on explicit naming conventions.

**Monorepo modifier:** Detected monorepo structure shifts `semantic_weight` down
by 0.1 due to embedding confusion from structurally similar subdirectories.

### 3. Self-Service Calibration (`bobbin calibrate`)

A user-facing tool that lets project owners find their optimal settings. Unlike
the eval framework (which needs curated ground-truth tasks), this uses the
project's own git history as ground truth.

```
$ bobbin calibrate

Calibrating search parameters against your git history...

  Strategy: For each of 20 sampled commits, use the commit message as a query
            and measure how well bobbin finds the files that were actually changed.

  Sampling: 20 commits across last 6 months (varied sizes, skip merge commits)
  Grid:     semantic_weight=[0.0, 0.3, 0.5, 0.7, 0.9], doc_demotion=[0.1, 0.3, 0.5]

  Running 300 probes... ████████████████████████ 100%

Results (top 5 by F1):
  sw=0.30 dd=0.30 k=60  F1=0.412  P=0.389  R=0.440
  sw=0.50 dd=0.30 k=60  F1=0.398  P=0.401  R=0.395
  sw=0.00 dd=0.30 k=60  F1=0.385  P=0.352  R=0.423
  sw=0.70 dd=0.30 k=60  F1=0.341  P=0.380  R=0.309
  sw=0.90 dd=0.30 k=60  F1=0.298  P=0.412  R=0.233

  Current config F1: 0.298 (sw=0.90)
  Best config F1:    0.412 (sw=0.30)  [+38% improvement]

  Apply best config? [Y/n]
```

**Key design decisions:**

**Ground truth from git history:** Each commit's diff-tree gives the files changed.
The commit message (plus PR title if available) is the query. This gives real
ground truth for the actual project without requiring curated eval tasks.

**Commit sampling strategy:**
- Skip merge commits, revert commits, and commits touching >30 files (refactors)
- Sample across time range (not just recent) to test recency effects
- Bias toward "interesting" commits: bug fixes, feature adds (heuristic from message)
- Require minimum 2 files changed (single-file commits are trivial)

**Parameter grid:** Smaller than the eval framework grid. Focus on the parameters
that matter most (semantic_weight, doc_demotion) with a coarser grid.

**Execution:** Uses the same `bobbin context --json` probe mechanism as the eval
framework. Single index, sweep all configs. With GPU: ~3 seconds per probe.
300 probes = ~15 minutes for a large repo.

### 4. Configuration Cascade

```
Highest priority
  ├── CLI flags (--semantic-weight 0.5)
  ├── .bobbin/config.toml [search] section (user-tuned)
  ├── bobbin calibrate results (if --apply was used)
  ├── bobbin inspect adaptive defaults (if --apply was used)
  └── Compiled defaults (current hardcoded values)
Lowest priority
```

`bobbin calibrate --apply` and `bobbin inspect --apply` both write to the same
`.bobbin/config.toml`. The user always has final control.

### 5. Recency & Coupling Exploration

The ablation showed neutral signal for both, but the parameter space is unexplored.
`bobbin calibrate` should also sweep these when `--full` is passed:

```
$ bobbin calibrate --full

Extended calibration (includes recency and coupling parameters)...

  Additional grid:
    recency_half_life_days: [7, 14, 30, 90]
    recency_weight: [0.0, 0.15, 0.30, 0.50]
    coupling_depth: [500, 2000, 5000, 20000]

  Running 2400 probes... ████████████ 45%
```

This requires re-indexing with different `coupling_depth` values, so it's
significantly slower. The `--full` flag makes the cost explicit.

## Implementation Plan

### Phase 1: `bobbin inspect` (Rust, low effort)
- Add `inspect` subcommand
- Count files by language using existing tree-sitter detection
- Estimate chunk count from file counts
- Detect monorepo structure (multiple build files at top level)
- Git history stats (age, frequency)
- Print recommended config
- `--apply` flag to write config.toml
- **Wire into `bobbin init`**: After creating default config, run inspect and
  offer to apply adaptive defaults

### Phase 2: `bobbin calibrate` (Rust + Python bridge, medium effort)
- Sample commits from git history
- Use `bobbin context --json` for probes (already exists)
- Parameter grid sweep with progress bar
- Report generation (terminal table + optional markdown)
- `--apply` flag to write config.toml
- Consider implementing in Rust directly (avoids Python dependency) since all
  the pieces (git log, bobbin context, scoring) are already in the binary

### Phase 3: Extended calibration (medium effort)
- Coupling depth sweep (requires re-index per depth value)
- Recency parameter sweep
- `--full` flag
- Cache intermediate results to allow resuming

### Phase 4: Integration (low effort)
- `bobbin init` runs inspect automatically, offers to apply
- `bobbin index` warns if config looks suboptimal for detected project size
- `bobbin status` shows current tier and whether calibration has been run

## Open Questions

1. **Should `bobbin calibrate` be Rust-native or Python?** Rust avoids the Python
   dependency but the eval framework is Python. Could ship as a separate
   `bobbin-eval` binary or keep it Rust-native with a simpler grid.

2. **How to handle projects that span multiple languages?** A repo with 50% Rust
   and 50% Python has conflicting optimal configs. Weight by file count? Or use
   a different strategy (language-aware scoring)?

3. **Should adaptive defaults update automatically?** After a major refactor that
   changes project size significantly, should `bobbin index` suggest re-running
   inspect/calibrate?

4. **Commit message quality as ground truth:** Some projects have terse commit
   messages ("fix bug") that make poor queries. Detect this and warn, or use
   diff content as a fallback query?

## Related

- `eval/results/ablation-flask-search.json` — Flask ablation data
- `eval/results/ablation-ruff-search.json` — Ruff ablation data
- `eval/runner/calibrate.py` — Existing calibration framework (eval-only)
- `src/config.rs` — Current defaults and config structures
- `docs/plans/eval-metrics-gate-tuning.md` — Related eval quality work
