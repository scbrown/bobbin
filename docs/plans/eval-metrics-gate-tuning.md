# Eval Framework Improvements: Metrics, Gate Tuning & Agent Guidance

## Context

Flask eval reruns showed bobbin **never injected context** (gate_skip on all 5 tasks, 0 injections, 0 manual bobbin commands). The agent gets the prime context but never uses bobbin CLI tools. We're also missing token usage, gate score details, and tool use traces — data needed to understand gaps and tune the system.

## Changes Overview

7 changes across 4 files + 1 new file:

1. **Capture token usage** from Claude Code JSON output
2. **Extract gate skip details** (actual scores, queries, thresholds)
3. **Lower default gate threshold** in eval settings (0.75 → 0.45)
4. **Add bobbin tool instructions** to agent prompt
5. **Capture tool use summary** from agent output
6. **Add token/cost columns** to report
7. **Add CLAUDE.md to eval workspace** instructing bobbin CLI usage

---

## 1. Capture Token Usage from Agent Output

**File:** `eval/runner/cli.py` — `_run_single()` ~line 299

The agent's JSON result already contains `total_cost_usd`, `usage` (input/output/cache tokens), and `modelUsage` per-model breakdown. We just discard it.

```python
# After line 303 (agent_result block), add token_usage extraction:
"token_usage": _extract_token_usage(agent_result.get("result")),
```

Add helper function:

```python
def _extract_token_usage(result: dict | None) -> dict | None:
    if not result or not isinstance(result, dict):
        return None
    usage = result.get("usage", {})
    return {
        "total_cost_usd": result.get("total_cost_usd"),
        "input_tokens": usage.get("input_tokens", 0),
        "output_tokens": usage.get("output_tokens", 0),
        "cache_creation_tokens": usage.get("cache_creation_input_tokens", 0),
        "cache_read_tokens": usage.get("cache_read_input_tokens", 0),
        "num_turns": result.get("num_turns"),
        "model_usage": result.get("modelUsage"),
    }
```

---

## 2. Extract Gate Skip Details

**File:** `eval/runner/cli.py` — `_read_bobbin_metrics()` ~line 368

Currently counts gate_skips but drops the actual scores. The metrics.jsonl events contain `metadata.top_score`, `metadata.gate_threshold`, and `metadata.query`.

```python
# Add after line 371 (gate_skips list):
gate_skip_details = []
for gs in gate_skips:
    meta = gs.get("metadata", {})
    gate_skip_details.append({
        "query": meta.get("query", ""),
        "top_score": meta.get("top_score"),
        "gate_threshold": meta.get("gate_threshold"),
    })

# Add to result dict:
"gate_skip_details": gate_skip_details,
```

---

## 3. Lower Gate Threshold in Eval Settings

**File:** `eval/settings-with-bobbin.json`

Add `--gate-threshold 0.45` to the inject-context hook command. This is more permissive than the default 0.75, allowing injection when there's even moderate semantic relevance. The actual scores from improvement #2 will let us tune this empirically.

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "command": "bobbin hook inject-context --gate-threshold 0.45",
        "statusMessage": "Loading code context...",
        "timeout": 10,
        "type": "command"
      }]
    }],
    "SessionStart": [{
      "hooks": [{
        "command": "bobbin hook prime-context",
        "timeout": 5,
        "type": "command"
      }]
    }]
  }
}
```

Why 0.45: This is well below the current 0.75 default. For a ~34K LOC Python repo, cosine similarities in the 0.4-0.7 range are common for related-but-not-exact queries. We'll collect the actual scores and tune from there.

---

## 4. Add Bobbin Instructions to Agent Prompt

**File:** `eval/runner/cli.py` — `_build_prompt()` ~line 62

Currently the prompt is minimal. For with-bobbin runs, append guidance so the agent knows to use bobbin tools. The approach-awareness is already in `_run_single` — we'll pass it through.

Update `_build_prompt` signature to accept `approach`:

```python
def _build_prompt(task: dict, approach: str = "no-bobbin") -> str:
    repo = task["repo"]
    desc = task["description"].strip()
    test_cmd = task["test_command"]
    base = (
        f"You are working on the {repo} project.\n\n"
        f"{desc}\n\n"
        f"Implement the fix. Run the test suite with `{test_cmd}` to verify."
    )
    if approach == "with-bobbin":
        base += (
            "\n\nThis project has bobbin installed (a code context engine). "
            "Use `bobbin search <query>` to find relevant code by meaning, "
            "`bobbin context <query>` for task-aware context assembly, "
            "and `bobbin related <file>` to discover co-changing files. "
            "These tools can help you navigate the codebase efficiently."
        )
    return base
```

Update call site (~line 236):
```python
prompt = _build_prompt(task, approach=approach)
```

---

## 5. Capture Tool Use Summary from Agent Output

**File:** `eval/runner/cli.py` — `_run_single()` ~line 299

The Claude Code JSON result includes `num_turns`. For deeper tool use tracking, we'd need `--verbose` mode which outputs the full conversation stream. For now, capture what's available from the standard JSON output and store the agent's `result` and `stderr` for post-hoc analysis.

Add to the result dict:
```python
"agent_output": {
    "num_turns": (agent_result.get("result") or {}).get("num_turns"),
    "session_id": (agent_result.get("result") or {}).get("session_id"),
    "stop_reason": (agent_result.get("result") or {}).get("stop_reason"),
},
```

Also capture stderr (contains bobbin hook output with gate scores):
```python
"agent_stderr": agent_result.get("stderr", "")[:5000],  # truncate for storage
```

---

## 6. Add Token/Cost Columns to Report

**File:** `eval/analysis/report.py`

Add Avg Cost and Avg Tokens to the summary table and per-task table.

In `_compute_approach_stats()` (~line 84), add:
```python
costs = [
    r["token_usage"]["total_cost_usd"]
    for r in results
    if r.get("token_usage", {}).get("total_cost_usd") is not None
]
input_toks = [
    r["token_usage"]["input_tokens"]
    for r in results
    if r.get("token_usage", {}).get("input_tokens") is not None
]
output_toks = [
    r["token_usage"]["output_tokens"]
    for r in results
    if r.get("token_usage", {}).get("output_tokens") is not None
]
# Add to return dict:
"avg_cost_usd": _safe_avg(costs),
"avg_input_tokens": _safe_avg(input_toks),
"avg_output_tokens": _safe_avg(output_toks),
```

Add to `_build_summary_table` metrics list:
```python
("Avg Cost ($)", "avg_cost_usd", False, False),
("Avg Input Tokens", "avg_input_tokens", False, False),
("Avg Output Tokens", "avg_output_tokens", False, False),
```

Add cost column to `_build_per_task_table`.

---

## 7. Add CLAUDE.md to Eval Workspace (with-bobbin only)

**New file:** `eval/workspace-claude-md.md` (template)

During bobbin setup, write a `.claude/CLAUDE.md` into the workspace to give the agent persistent instructions about bobbin.

**File:** `eval/runner/bobbin_setup.py` — `setup_bobbin()` ~line 97 (after init, before index)

```python
# Write workspace CLAUDE.md for agent guidance
claude_dir = ws / ".claude"
claude_dir.mkdir(exist_ok=True)
claude_md = claude_dir / "CLAUDE.md"
claude_md.write_text(_WORKSPACE_CLAUDE_MD)
```

Template content:
```markdown
# Project Tools

This project is indexed by **bobbin**, a code context engine.
Use these commands to explore the codebase:

- `bobbin search <query>` — find code by meaning (semantic search)
- `bobbin context <query>` — get a focused context bundle for a task
- `bobbin related <file>` — find files that frequently change together
- `bobbin refs <symbol>` — find definitions and usages of a symbol
- `bobbin grep <pattern>` — regex/keyword search across all files

Prefer bobbin tools over manual grep/find for navigating unfamiliar code.
```

---

## Note: Pre-indexing Timing

Indexing already happens before the agent runs (in `setup_bobbin()`, called before `run_agent()`). The report's Duration column is purely agent time (`agent_result.duration_seconds`), not including index time. Index timing is tracked separately in `bobbin_metadata.index_duration_seconds`. No change needed here — the current architecture is correct.

---

## Files Modified

| File | Changes |
|------|---------|
| `eval/runner/cli.py` | Token extraction, gate details, prompt approach param, agent output capture |
| `eval/runner/bobbin_setup.py` | Write CLAUDE.md into workspace |
| `eval/settings-with-bobbin.json` | Lower gate threshold to 0.45 |
| `eval/analysis/report.py` | Add cost/token columns to summary + per-task tables |
| `eval/workspace-claude-md.md` | **New** — CLAUDE.md template for eval workspaces |

## Verification

1. Run a single flask task with-bobbin: `just eval-task flask-001 --approaches with-bobbin`
2. Check result JSON for new fields: `token_usage`, `gate_skip_details`, `agent_output`, `agent_stderr`
3. Verify gate_skip_details shows actual scores (should be < 0.45 if still skipping, or injection_count > 0 if gate passes)
4. Verify agent stderr contains bobbin hook output
5. Generate report and confirm cost/token columns appear
6. Check if agent invoked any bobbin commands (look at `bobbin_metrics.command_invocations`)
