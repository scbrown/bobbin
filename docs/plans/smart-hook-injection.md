# Smart Hook Context Injection

**Bead**: bo-slk
**Status**: Planning
**Priority**: P1

## Context

Bobbin's Claude Code hook injects relevant code context on every prompt. Three production problems observed:

1. **Conversational prompts get noisy context** — "how was bobbin submitting context?" triggers 150 lines of marginal code. The per-result threshold (0.5) filters individual chunks but doesn't suppress the entire injection when nothing is truly relevant.
2. **Repeated context across prompts** — same topic = same chunks injected every prompt, wasting tokens when the model already has them.
3. **No visibility** into what code keeps getting surfaced — no way to see patterns or pin frequently-referenced code.

**Key technical finding**: RRF normalization (`context.rs:390-400`) divides all scores by the max score, so the top result always = 1.0. A gate threshold on normalized scores would never fire. The gate must use **raw cosine similarity** from the semantic search, captured before RRF.

## Design Principles

- **Never block the user's prompt** — all failures → silent exit, errors to stderr
- **State file operations must be fast** — no file locking, JSON not SQLite
- **Two thresholds, two jobs** — gate_threshold decides "inject at all?", threshold filters individual results
- **Session = topic, not prompt** — same code area = same session ID regardless of prompt wording

## Configuration

### `.bobbin/config.toml`

```toml
[hooks]
threshold = 0.5           # Per-result filter on normalized RRF scores (existing)
budget = 150              # Max lines of injected context (existing)
content_mode = "preview"  # full | preview | none (existing)
min_prompt_length = 10    # Skip injection for very short prompts (existing)
gate_threshold = 0.75     # NEW: Min raw semantic similarity to inject at all
dedup_enabled = true      # NEW: Skip injection when search results haven't changed
```

### Implementation in `src/config.rs`

Add to `HooksConfig` struct (line ~233):

```rust
pub struct HooksConfig {
    pub threshold: f32,           // 0.5
    pub budget: usize,            // 150
    pub content_mode: String,     // "preview"
    pub min_prompt_length: usize, // 10
    pub gate_threshold: f32,      // NEW: 0.75
    pub dedup_enabled: bool,      // NEW: true
}
```

All fields use `#[serde(default)]` for backward compatibility with existing configs.

---

## 1. Top-Score Gate

### Problem

The search always returns results. With RRF normalization, the top result always scores 1.0. The existing per-result threshold (0.5) cannot suppress the entire injection — it only filters individual chunks after the decision to inject is already made.

### Solution

Capture the **raw cosine similarity** of the top semantic result before RRF normalization. Gate the entire injection on this value.

### Implementation

**`src/search/context.rs`**:

1. Add `top_semantic_score: f32` to `ContextSummary` (line ~76)
2. In `run_hybrid_search()` (line ~311), before the RRF loop at line 338:
   ```rust
   let top_semantic_score = semantic_results.first()
       .map(|r| r.score)
       .unwrap_or(0.0);
   ```
3. Propagate `top_semantic_score` through `assemble()` into the `ContextBundle.summary`

**`src/cli/hook.rs`**:

1. Add `--gate-threshold` CLI arg to `InjectContextArgs` (line ~75)
2. In `inject_context_inner()`, after bundle assembly (~line 620):
   ```rust
   let gate = args.gate_threshold.unwrap_or(config.hooks.gate_threshold);
   if bundle.summary.top_semantic_score < gate {
       eprintln!("bobbin: skipped (semantic={:.2} < gate={:.2})",
           bundle.summary.top_semantic_score, gate);
       return Ok(());
   }
   ```

---

## 2. Session-Aware Dedup

### Problem

When working on the same topic across multiple prompts, bobbin injects the same chunks every time. The model already has them in context from 3 prompts ago.

### Solution

Compute a "session ID" from the search results. If the session ID hasn't changed since the last injection, skip — the model already has this context.

### State File Schema

`.bobbin/hook_state.json`:

```json
{
  "last_session_id": "a1b2c3d4e5f6a7b8",
  "last_injected_chunks": ["src/foo.rs:10:50", "src/bar.rs:20:40"],
  "last_injection_time": "2026-02-08T10:30:00Z",
  "injection_count": 47,
  "chunk_frequencies": {
    "src/foo.rs:10:50": { "count": 12, "file": "src/foo.rs", "name": "InjectContextArgs" },
    "src/bar.rs:20:40": { "count": 9, "file": "src/bar.rs", "name": "HooksConfig" }
  },
  "file_frequencies": {
    "src/foo.rs": 15,
    "src/bar.rs": 12
  },
  "hot_topics_generated_at": 40
}
```

### Session ID Algorithm

1. Collect chunk composite keys from bundle: `format!("{}:{}:{}", file.path, chunk.start_line, chunk.end_line)`
2. Filter by per-result threshold
3. Sort alphabetically, take top 10
4. Concatenate with `|` separator
5. SHA-256 hash, take first 16 hex chars

Uses `sha2` and `hex` crates (both already dependencies).

### Implementation

**`src/cli/hook.rs`**:

1. Add types:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, Default)]
   struct HookState {
       last_session_id: String,
       last_injected_chunks: Vec<String>,
       last_injection_time: String,
       injection_count: u64,
       chunk_frequencies: HashMap<String, ChunkFrequency>,
       file_frequencies: HashMap<String, u64>,
       hot_topics_generated_at: u64,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   struct ChunkFrequency {
       count: u64,
       file: String,
       name: Option<String>,
   }
   ```

2. Add helper functions:
   - `load_hook_state(repo_root: &Path) -> HookState` — reads JSON, defaults on error
   - `save_hook_state(repo_root: &Path, state: &HookState)` — writes JSON, swallows errors
   - `compute_session_id(bundle: &ContextBundle, threshold: f32) -> String`

3. Add `--no-dedup` flag to `InjectContextArgs`

4. In `inject_context_inner()`, after gate check:
   ```
   if dedup_enabled:
     load state
     compute session_id
     if session_id == state.last_session_id → return Ok(())
   ...inject...
   update state (session_id, chunks, timestamps, frequencies)
   save state
   ```

---

## 3. Hot Topics File

### Problem

No visibility into what code bobbin keeps surfacing. Frequently-injected chunks waste budget on every prompt when they could be pinned as persistent context.

### Solution

Auto-generate `.bobbin/hot-topics.md` from injection frequency data. Useful as:
- Human-readable reference for what code is "hot"
- Candidate for CLAUDE.md inclusion or session-context pinning
- Input for future automatic suppression of over-injected chunks

### Format

```markdown
# Hot Topics (auto-generated by bobbin)

Last updated: 2026-02-08 10:30 UTC
Based on 47 context injections.

## Frequently Referenced Code

| Rank | File | Symbol | Injections |
|------|------|--------|------------|
| 1 | src/cli/hook.rs | InjectContextArgs | 12 |
| 2 | src/config.rs | HooksConfig | 9 |
| 3 | src/search/context.rs | ContextAssembler | 7 |

## Most Referenced Files

| File | Total Injections |
|------|-----------------|
| src/cli/hook.rs | 15 |
| src/config.rs | 12 |
| src/search/context.rs | 9 |

## Notes

- Chunks appearing here are candidates for pinning in CLAUDE.md or session context.
- Regenerated every 10 injections. Run `bobbin hook hot-topics` to force refresh.
```

### Implementation

**`src/cli/hook.rs`**:

1. Add `HotTopics(HotTopicsArgs)` to `HookCommands` enum
2. Add `generate_hot_topics(state: &HookState, output_path: &Path) -> Result<()>`
   - Sort `chunk_frequencies` by count descending, take top 20
   - Sort `file_frequencies` by count descending, take top 10
   - Format as markdown tables
3. In state update path (after successful injection), trigger when `injection_count % 10 == 0`
4. CLI: `bobbin hook hot-topics [--force]`

---

## File Structure

### Modified files

```
src/search/context.rs        # Add top_semantic_score to ContextSummary, capture in run_hybrid_search
src/config.rs                # Add gate_threshold, dedup_enabled to HooksConfig
src/cli/hook.rs              # Gate check, dedup logic, state management, hot-topics subcommand
docs/configuration.md        # Document new [hooks] config values
```

No new source files — all logic fits in existing hook.rs alongside current implementation.

---

## Bead Breakdown

### bo-slk-1: Top-score gate (gate_threshold)
- Add `top_semantic_score: f32` to `ContextSummary` in context.rs
- Capture `semantic_results[0].score` before RRF in `run_hybrid_search()`
- Add `gate_threshold: f32` to `HooksConfig` (default 0.75)
- Add `--gate-threshold` CLI arg to `InjectContextArgs`
- Gate check in `inject_context_inner()`: skip if top_semantic_score < gate
- Update `HookStatusOutput` to include gate_threshold
- **Acceptance**: `--gate-threshold 0.99` skips; `--gate-threshold 0.0` injects; status shows value; unit tests

### bo-slk-2: Session-aware dedup
- Add `HookState`, `ChunkFrequency` types (Serialize/Deserialize)
- Add `load_hook_state()`, `save_hook_state()`, `compute_session_id()`
- Add `dedup_enabled: bool` to `HooksConfig` (default true)
- Add `--no-dedup` flag to `InjectContextArgs`
- Dedup check in `inject_context_inner()`: skip if session_id matches
- After injection: update state with frequencies, timestamps, session_id
- **Acceptance**: First query writes state; repeat skips; different query injects; `--no-dedup` forces; corrupt state falls back to default

### bo-slk-3: Hot topics file
- Add `HotTopics(HotTopicsArgs)` to `HookCommands`
- Add `generate_hot_topics()` function
- Trigger every 10 injections in state update path
- CLI: `bobbin hook hot-topics [--force]`
- **Acceptance**: After 10 injections, `.bobbin/hot-topics.md` exists; `bobbin hook hot-topics` works; empty state produces valid markdown

### bo-slk-4: Status updates + polish
- Update `run_status` to show gate_threshold, dedup status, injection count
- Update `docs/configuration.md` with all new config values
- **Acceptance**: Status shows all new fields in human + JSON modes; all existing tests pass

---

## Dependency Order

```
bo-slk-1 (gate threshold)
    ↓
bo-slk-2 (session dedup)
    ↓
bo-slk-3 (hot topics)
    ↓
bo-slk-4 (status + polish)
```

Sequential: each builds on the prior. 1 must come first (context.rs changes). 2 adds state tracking that 3 depends on.

---

## Verification

End-to-end test:
1. `bobbin hook inject-context --gate-threshold 0.99` with any query → silent
2. `bobbin hook inject-context` with code query → injects, writes `hook_state.json`
3. Same query again → skipped (dedup)
4. Different query → injects, state updated
5. `bobbin hook inject-context --no-dedup` → forces injection
6. After 10 injections → `hot-topics.md` generated
7. `bobbin hook hot-topics --force` → regenerates from state
8. `bobbin hook status` → shows gate, dedup, injection count
9. All existing tests pass (`cargo test`)
