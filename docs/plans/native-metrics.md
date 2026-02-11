# Plan: Bobbin Native Metrics & Eval Observability

## Context

Bobbin evals have near-zero observability. We can't tell if hooks fire, if the agent uses bobbin tools, or if injected context points at the right files. The fix belongs in **bobbin itself** — structured metrics emitted to `.bobbin/metrics.jsonl` for every command and hook event. The eval framework then reads this file after the run.

Additionally, the eval agent doesn't know bobbin exists as a tool — it only receives passive hook injections. Adding a SessionStart hook with `bobbin prime` teaches the agent about available commands.

## Part 1: Metrics Source Identity

Every metric event needs a `source` field identifying who generated it. Claude Code passes `session_id` in the JSON stdin to every hook event — this is the canonical identifier.

**Resolution chain (highest priority first):**
1. **`--metrics-source` CLI flag** — global flag on any bobbin command
2. **`BOBBIN_METRICS_SOURCE` env var** — set by eval runner or wrapper scripts
3. **Claude Code `session_id`** — parsed from hook stdin JSON (already sent, currently ignored)

The `source` field is **required** — if none of the above resolve, bobbin should not emit metrics (or use a fallback like `"unknown"`). In practice, hooks always get `session_id` from Claude Code, and eval sets the env var.

**Changes to hook input structs** (`src/cli/hook.rs`):

`HookInput` (line 501) — add `session_id`:
```rust
struct HookInput {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    session_id: String,
}
```

`SessionStartInput` (line 962) — add `session_id`:
```rust
struct SessionStartInput {
    #[serde(default)]
    source: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    session_id: String,
}
```

**Source resolution function** (`src/metrics.rs`):
```rust
pub fn resolve_source(cli_flag: Option<&str>, env_var: Option<&str>, hook_session_id: Option<&str>) -> String {
    cli_flag
        .or(env_var)
        .or(hook_session_id)
        .unwrap_or("unknown")
        .to_string()
}
```

For non-hook commands (e.g., `bobbin search`), there's no stdin session_id, so the resolution is: CLI flag > env var > "unknown".

## Part 2: Metrics Infrastructure

### New module: `src/metrics.rs`

Append-only JSONL at `.bobbin/metrics.jsonl`.

**Event structure:**
```rust
#[derive(Serialize, Deserialize)]
pub struct MetricEvent {
    pub timestamp: String,         // RFC3339
    pub source: String,            // Session/caller identity
    pub event_type: String,        // "command" | "hook_injection" | "hook_gate_skip" | "hook_dedup_skip"
    pub command: String,           // "search" | "context" | "hook inject-context" | etc.
    pub duration_ms: u64,
    pub metadata: serde_json::Value,
}
```

**API:**
```rust
pub fn emit(repo_root: &Path, event: MetricEvent)          // append one JSONL line
pub fn read_all(repo_root: &Path) -> Vec<MetricEvent>       // read entire log
pub fn read_by_source(repo_root: &Path, source: &str) -> Vec<MetricEvent>  // filter by source
pub fn clear(repo_root: &Path)                               // reset
```

`emit()` opens file in append mode, writes one JSON line, closes. No locking needed.

### Command-level metrics: `src/cli/mod.rs`

Add `--metrics-source` as a global CLI flag on the `Cli` struct (~line 117):
```rust
#[arg(long, global = true, env = "BOBBIN_METRICS_SOURCE")]
metrics_source: Option<String>,
```

Wrap the dispatch match in `Cli::run()` (~line 148):
```rust
let start = std::time::Instant::now();
let command_name = self.command.name();
let source = self.metrics_source.clone();

let result = match self.command { ... };

// Best-effort metric emission
if let Some(root) = find_repo_root() {
    let _ = metrics::emit(&root, MetricEvent {
        source: metrics::resolve_source(source.as_deref(), None, None),
        event_type: "command".into(),
        command: command_name,
        duration_ms: start.elapsed().as_millis() as u64,
        ..
    });
}
result
```

### Hook-level metrics: `src/cli/hook.rs`

In `inject_context_inner()`, emit events at each decision point:

- **Successful injection** (~line 900): emit `hook_injection` with `{query, files_returned: [...], chunks_returned, top_score, budget_lines_used}`
- **Gate skip** (~line 874): emit `hook_gate_skip` with `{query, top_score, gate_threshold}`
- **Dedup skip** (~line 887): emit `hook_dedup_skip` with `{query}`

Source for hooks: `resolve_source(cli_flag, env_var, Some(&input.session_id))`

Keep existing `eprintln!` messages alongside metric events.

## Part 3: SessionStart Hook for Bobbin Prime

### New subcommand: `bobbin hook prime-context`

**File: `src/cli/hook.rs`**

Reads stdin JSON (gets `session_id`, `source`), outputs `hookSpecificOutput` JSON with bobbin primer + live stats:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "additionalContext": "# Bobbin - Code Context Engine\n\n[brief primer content]\n\n## Index Status\n- 4950 files, 57533 chunks indexed\n- Languages: Rust (1750 files), Python (3690 files)\n\n## Available Commands\n- `bobbin search <query>` ...\n..."
  }
}
```

Uses existing `PRIMER` constant and `extract_brief()` from `prime.rs`, plus `VectorStore::get_stats()` for live data.

Also emits a `hook_prime_context` metric event.

### Update eval settings

**File: `eval/settings-with-bobbin.json`**

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [{
          "command": "bobbin hook inject-context",
          "timeout": 10,
          "type": "command"
        }]
      }
    ],
    "SessionStart": [
      {
        "hooks": [{
          "command": "bobbin hook prime-context",
          "timeout": 5,
          "type": "command"
        }]
      }
    ]
  }
}
```

**Risk**: SessionStart may not fire in `-p` mode. Fallback: write primer to `.claude/CLAUDE.md` during `setup_bobbin()`.

## Part 4: Eval Framework Reads Metrics

### After agent run: `eval/runner/cli.py`

Set `BOBBIN_METRICS_SOURCE` in the agent's environment before launching:
```python
env["BOBBIN_METRICS_SOURCE"] = f"{task_id}_{approach}_{attempt}"
```

After the agent finishes, read and summarize `.bobbin/metrics.jsonl`:
```python
bobbin_metrics = None
if approach == "with-bobbin":
    metrics_path = ws / ".bobbin" / "metrics.jsonl"
    if metrics_path.exists():
        source_tag = f"{task_id}_{approach}_{attempt}"
        events = [json.loads(l) for l in metrics_path.read_text().splitlines() if l.strip()]
        # Filter to our source (in case workspace is reused)
        events = [e for e in events if e.get("source") == source_tag]

        injections = [e for e in events if e["event_type"] == "hook_injection"]
        gate_skips = [e for e in events if e["event_type"] == "hook_gate_skip"]
        dedup_skips = [e for e in events if e["event_type"] == "hook_dedup_skip"]
        commands = [e for e in events if e["event_type"] == "command"]

        injected_files = set()
        for inj in injections:
            for f in inj.get("metadata", {}).get("files_returned", []):
                injected_files.add(f)

        bobbin_metrics = {
            "injection_count": len(injections),
            "gate_skip_count": len(gate_skips),
            "dedup_skip_count": len(dedup_skips),
            "command_invocations": [{"command": e["command"], "duration_ms": e["duration_ms"]} for e in commands],
            "injected_files": sorted(injected_files),
            "raw_events": events,
        }
```

### Injection-to-ground-truth overlap

```python
        if injected_files and diff_result:
            gt_files = set(diff_result.get("ground_truth_files", []))
            overlap = injected_files & gt_files
            bobbin_metrics["overlap"] = {
                "injection_precision": round(len(overlap) / len(injected_files), 4),
                "injection_recall": round(len(overlap) / len(gt_files), 4),
                "overlap_files": sorted(overlap),
            }
```

Add `"bobbin_metrics": bobbin_metrics` to result dict.

## Files Modified

| File | Changes |
|------|---------|
| `src/metrics.rs` | **New**: MetricEvent, emit/read/read_by_source/clear |
| `src/lib.rs` | Register `pub mod metrics` |
| `src/cli/mod.rs` | Add `--metrics-source` global flag, wrap dispatch with metric emission |
| `src/cli/hook.rs` | Add `session_id` to HookInput/SessionStartInput, emit metric events, add `prime-context` subcommand |
| `eval/runner/cli.py` | Set BOBBIN_METRICS_SOURCE env var, read metrics.jsonl, compute overlap |
| `eval/runner/agent_runner.py` | Pass env dict to subprocess.run |
| `eval/settings-with-bobbin.json` | Add SessionStart hook |

## Metrics Summary

| Metric | Source | Insight |
|--------|--------|---------|
| `injection_count` | metrics.jsonl | Is bobbin actually injecting? |
| `gate_skip_count` | metrics.jsonl | How often is context irrelevant? |
| `dedup_skip_count` | metrics.jsonl | How often is context repeated? |
| `injection_precision` | overlap calc | Are injected files the right ones? |
| `injection_recall` | overlap calc | Does bobbin find ground truth files? |
| `command_invocations` | metrics.jsonl | Does agent use bobbin tools? |
| `top_score` per injection | event metadata | Confidence in results |
| `files_returned` per injection | event metadata | Exactly what was injected |

## Verification

1. `cargo test` — metrics module unit tests
2. `bobbin search "test"` → `.bobbin/metrics.jsonl` has command event
3. Manual hook test: `echo '{"prompt":"test","cwd":".","session_id":"abc"}' | bobbin hook inject-context` → metrics file has event with source "abc"
4. `BOBBIN_METRICS_SOURCE=eval-test bobbin search "test"` → event source is "eval-test"
5. `just eval-task ruff-005` → result JSON has `bobbin_metrics` with injection/overlap data
6. Test SessionStart hook in `-p` mode
