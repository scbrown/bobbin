# Design: Adaptive Defaults — Project-Aware Search Configuration

**Status**: Approved (decisions finalized 2026-02-25)

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

### Key Decisions

<!-- human-approved: stiwi 2026-02-25 -->

1. **Merge inspect and calibrate.** No separate `bobbin inspect` phase. Calibrate
   runs automatically after `bobbin index` and empirically finds optimal config.
   Project profile info (language distribution, chunk count, etc.) becomes
   diagnostic output in `bobbin status`.

2. **Rust-native implementation.** No Python dependency. All pieces (git log
   parsing, context probing, scoring) already exist in the binary.

3. **Single-language scope.** Multi-language repos (50% Rust + 50% Python) are
   backlogged. v1 treats the project as one unit. See [Backlog](#backlog).

4. **Auto-recalibrate on significant change.** After indexing, bobbin checks
   whether the project has changed enough since last calibration to warrant
   re-running. The "what changed" detection is behind a trait so it's easy to
   extend over time. See [Change Detection](#change-detection).

5. **Warn on terse commit messages.** If sampled commits have low-quality messages
   (short, generic), warn the user that calibration accuracy may be reduced.
   Fallback strategies (diff-content queries, PR titles) are backlogged.
   See [Backlog](#backlog).

### 1. Auto-Calibration (`bobbin calibrate`)

Runs automatically at the end of `bobbin index` (skippable with `--skip-calibrate`).
Also available standalone for manual tuning.

```
$ bobbin index
Indexing 4,940 files...
  Chunks: 57,158  Embeddings: ████████████████████████ done

Calibrating search parameters against git history...
  Sampling: 20 commits across last 6 months
  Grid:     semantic_weight=[0.0, 0.3, 0.5, 0.7, 0.9], doc_demotion=[0.1, 0.3, 0.5]

  Running 300 probes... ████████████████████████ 100%

Calibration results (top 3 by F1):
  sw=0.30 dd=0.30 k=60  F1=0.412  P=0.389  R=0.440
  sw=0.50 dd=0.30 k=60  F1=0.398  P=0.401  R=0.395
  sw=0.00 dd=0.30 k=60  F1=0.385  P=0.352  R=0.423

  Previous config F1: 0.298 (sw=0.90)
  Best config F1:     0.412 (sw=0.30)  [+38% improvement]

  ✓ Applied best config to .bobbin/config.toml
```

**Ground truth from git history:** Each commit's diff-tree gives the files changed.
The commit message is the query. This gives real ground truth for the actual
project without requiring curated eval tasks.

**Commit sampling strategy:**
- Skip merge commits, revert commits, and commits touching >30 files (refactors)
- Sample across time range (not just recent) to test recency effects
- Bias toward "interesting" commits: bug fixes, feature adds (heuristic from message)
- Require minimum 2 files changed (single-file commits are trivial)

**Terse message detection:** If >50% of sampled commits have messages under 20
characters or match generic patterns ("fix", "update", "wip"), emit a warning:
```
⚠ Many commit messages are too short for reliable calibration.
  Calibration accuracy may be reduced. Consider running with --verbose
  to inspect which commits were sampled.
```

**Parameter grid:** Focus on the parameters that matter most (`semantic_weight`,
`doc_demotion`) with a coarse grid. With GPU: ~3 seconds per probe.
300 probes = ~15 minutes for a large repo.

**Execution:** Probes use the existing `ContextAssembler::assemble()` path
internally (no subprocess overhead). Single index, sweep all configs by
overriding `ContextConfig` fields per probe.

### 2. Change Detection

After indexing, bobbin decides whether to run calibration by consulting a
`CalibrationGuard` trait:

```rust
/// Determines whether calibration should run after an index operation.
trait CalibrationGuard {
    /// Returns true if the project has changed enough to warrant recalibration.
    fn should_recalibrate(&self, current: &ProjectSnapshot, previous: &ProjectSnapshot) -> bool;
}

/// Point-in-time snapshot of project characteristics relevant to calibration.
struct ProjectSnapshot {
    chunk_count: usize,
    file_count: usize,
    primary_language: String,
    language_distribution: Vec<(String, f32)>,  // (language, fraction)
    repo_age_days: u32,
    recent_commit_rate: f32,  // commits/week over last 30 days
    calibrated_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

**v1 implementation** (`DefaultCalibrationGuard`):
- Recalibrate if no previous calibration exists
- Recalibrate if chunk count changed by >20%
- Recalibrate if primary language changed
- Recalibrate if last calibration was >30 days ago

The trait boundary makes it easy to add richer heuristics later (monorepo
detection, identifier density shifts, etc.) without changing the index pipeline.

The `ProjectSnapshot` is persisted in `.bobbin/calibration.json` alongside the
calibration results.

### 3. Configuration Cascade

```
Highest priority
  ├── CLI flags (--semantic-weight 0.5)
  ├── .bobbin/config.toml [search] section (user-tuned)
  ├── Calibration results (.bobbin/calibration.json)
  └── Compiled defaults (current hardcoded values)
Lowest priority
```

`bobbin calibrate` writes to `.bobbin/calibration.json` (not config.toml).
This keeps auto-tuned values separate from explicit user overrides. If the user
sets `semantic_weight = 0.5` in config.toml, that always wins over calibration.

### 4. Extended Calibration (`--full`)

Sweeps recency and coupling parameters in addition to the core grid:

```
$ bobbin calibrate --full

Extended calibration (includes recency and coupling parameters)...

  Additional grid:
    recency_half_life_days: [7, 14, 30, 90]
    recency_weight: [0.0, 0.15, 0.30, 0.50]
    coupling_depth: [500, 2000, 5000, 20000]

  Running 2400 probes... ████████████ 45%
```

Coupling depth sweep requires re-indexing coupling data per depth value, so it's
significantly slower. The `--full` flag makes the cost explicit.

### 5. Project Profile in `bobbin status`

The old "inspect" diagnostics surface here instead of as a separate command:

```
$ bobbin status

Index
  Chunks:      57,158
  Files:       4,940
  Languages:   rust (68%), python (23%), markdown (9%)
  Last indexed: 2 hours ago

Calibration
  Status:      calibrated (2026-02-25)
  Config:      sw=0.30 dd=0.30 k=60 (F1=0.412)
  Stale:       no (chunk delta: +2%)

Git
  Repo age:    3.2 years
  Commit rate: 12.4 commits/week
  Structure:   monorepo (47 top-level crates)
```

## Implementation Plan

### Phase 1: `bobbin calibrate` (Rust, medium effort)
- Commit sampler: git log parsing, filtering, stratified sampling
- Probe runner: `ContextAssembler` with overridden config per probe
- Scorer: precision/recall/F1 (file-level, matching eval framework)
- Grid sweep with progress bar (indicatif)
- Results output: terminal table + `.bobbin/calibration.json`
- `ProjectSnapshot` capture and persistence
- Terse message detection + warning

### Phase 2: Auto-calibration integration (low effort)
- `CalibrationGuard` trait + `DefaultCalibrationGuard`
- Wire into `bobbin index`: after indexing, check guard, run calibrate if needed
- `--skip-calibrate` flag on `bobbin index`
- Config cascade: calibration.json read at search time

### Phase 3: Extended calibration (medium effort)
- Coupling depth sweep (requires re-index per depth value)
- Recency parameter sweep
- `--full` flag
- Cache intermediate results to allow resuming

### Phase 4: Status integration (low effort)
- `bobbin status` shows project profile + calibration state
- Staleness indicator (how much has changed since last calibration)

## Backlog

Items explicitly deferred from v1. Track these as future beads when the
foundation is stable.

### Multi-language project handling
**Context**: A repo with 50% Rust and 50% Python has conflicting optimal configs.
Calibrate v1 treats the project as one unit. Future options:
- Weight by file count in calibration scoring
- Language-aware scoring (different weights per language at query time)
- Per-directory config overrides (monorepo crate-level tuning)

### Terse commit message fallbacks
**Context**: Projects with poor commit messages ("fix bug", "update") produce
unreliable calibration. v1 warns but doesn't compensate. Future options:
- Use diff content (added lines) as fallback query material
- Extract PR titles from GitHub/Forgejo API
- Use file paths + function names from diff as structured query
- Filter terse commits out entirely and require a minimum viable sample size

### Dependency-graph expansion signal
**Context**: The implemented `deps` feature (import graph) could be used as a
new expansion signal in `ContextAssembler` alongside coupling and bridging.
Not wired into injection yet — would help full agentic runs but not the
search-probe calibration. Track as separate work.

## Related

- `eval/results/ablation-flask-search.json` — Flask ablation data
- `eval/results/ablation-ruff-search.json` — Ruff ablation data
- `eval/runner/calibrate.py` — Existing calibration framework (eval-only)
- `src/config.rs` — Current defaults and config structures
- `src/search/context.rs` — Context assembler (probe target)
- `docs/plans/eval-metrics-gate-tuning.md` — Related eval quality work
