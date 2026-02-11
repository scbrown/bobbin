# Eval: Collect Claude Code Tool Use Traces via stream-json

## Context

The bobbin eval framework runs Claude Code headless on coding tasks and compares results with/without bobbin. Currently it uses `--output-format json` which returns only a final summary (cost, tokens, num_turns). We have **zero visibility** into agent behavior: which tools it calls, how many times, whether it uses bobbin CLI commands, how quickly it starts editing.

Flask evals showed bobbin never injected context AND the agent never used bobbin CLI tools — but we couldn't tell what the agent *was* doing instead. Tool use traces fill this gap.

## Key Discovery

Claude Code's `--output-format stream-json --verbose` emits every conversation message as JSONL:

```
{"type":"system","subtype":"init","tools":["Bash","Read","Grep","Edit",...],...}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git status"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_xxx","content":"..."}]}}
...
{"type":"result","subtype":"success","total_cost_usd":0.085,"num_turns":3,"usage":{...},"modelUsage":{...}}
```

The final `"type":"result"` line contains the same data as `--output-format json`, so switching is backward-compatible.

## Changes

### 1. JSONL Stream Parser (`eval/runner/agent_runner.py`)

New function `parse_stream_json(raw_output: str) -> tuple[dict | None, dict]`:
- Iterates lines, parses each as JSON, skips blanks/unparseable
- Tracks turn counter (increments per `type:assistant` message)
- For each `type:assistant` message, scans `message.content` for `type:tool_use` items:
  - Counts by tool name (`by_tool`)
  - Appends to ordered `tool_sequence`
  - Records `first_edit_turn` (first Edit/Write call)
  - Detects bobbin CLI invocations (Bash inputs containing "bobbin")
- Extracts final `type:result` line as the summary dict
- Returns `(result_summary, tool_use_summary)`

Tool use summary schema:
```python
{
    "by_tool": {"Bash": 12, "Read": 5, "Edit": 3, ...},
    "total_tool_calls": 24,
    "bobbin_commands": [{"command": "bobbin search auth", "turn": 2}],
    "first_edit_turn": 7,       # None if no Edit/Write used
    "tool_sequence": ["Read", "Grep", "Bash", "Edit", ...],
}
```

### 2. Switch run_agent() to stream-json (`eval/runner/agent_runner.py`)

Change command flags (line 79):
```python
# Before:
"--output-format", "json",
# After:
"--output-format", "stream-json",
"--verbose",
```

Replace JSON parsing block (lines 125-131) with `parse_stream_json()` call. Add `tool_use_summary` to return dict. Fallback: if no `type:result` line found, try `json.loads(stdout)` for backward compat with older Claude CLI.

### 3. Store tool_use_summary + save raw stream (`eval/runner/cli.py`)

In `_run_single()` result dict (~line 335), add:
```python
"tool_use_summary": agent_result.get("tool_use_summary"),
```

New helper `_save_raw_stream()` saves `output_raw` as `<task>_<approach>_<attempt>.stream.jsonl` alongside the result JSON. Best-effort, never raises.

Add `--save-stream / --no-save-stream` flag to `run_task` and `run_all` (default: True).

### 4. Report columns (`eval/analysis/report.py`)

Extend `_compute_approach_stats()` to extract from `tool_use_summary`:
- `avg_tool_calls` — average total tool calls per run
- `avg_first_edit_turn` — how quickly agent starts editing
- `avg_bobbin_commands` — average bobbin CLI invocations per run

Add rows to `_build_summary_table()`:
```python
("Avg Tool Calls", "avg_tool_calls", False, False),
("Avg First Edit Turn", "avg_first_edit_turn", False, False),
("Avg Bobbin Commands", "avg_bobbin_commands", False, False),
```

Add `Tools` column to `_build_per_task_table()`.

Backward compat: `r.get("tool_use_summary", {}).get(...)` pattern means old results without this field produce empty lists and 0.0 averages.

### 5. Tests

**`test_agent_runner.py`**:
- New `TestParseStreamJson` class: basic stream, bobbin command detection, empty output, no result line, partial/corrupt stream
- Update existing `TestRunAgent`: mock stdout as JSONL, verify `tool_use_summary` in return dict, verify command contains `stream-json` not `json`

**`test_cli.py`**:
- Add `tool_use_summary` to `_make_result()` fixture helper
- Test that raw stream file is saved

**`test_report.py`**:
- Verify tool use columns appear in generated report
- Backward compat test: results without `tool_use_summary` still generate reports

## Files Modified

| File | Change |
|------|--------|
| `eval/runner/agent_runner.py` | Add `parse_stream_json()`, switch to stream-json, add `tool_use_summary` to return |
| `eval/runner/cli.py` | Store `tool_use_summary` in result, add `_save_raw_stream()`, add `--save-stream` flag |
| `eval/analysis/report.py` | Add tool use metrics to summary + per-task tables |
| `eval/tests/test_agent_runner.py` | Test JSONL parser, update command assertions |
| `eval/tests/test_cli.py` | Update fixtures with `tool_use_summary` |

## Bead Breakdown

| Bead | Title | Depends On |
|------|-------|------------|
| 1 | stream-json parser + run_agent switch | — |
| 2 | Store tool_use_summary + save raw stream | 1 |
| 3 | Report tool use columns | 2 |

## Verification

1. `cd eval && python -m pytest tests/ -v` — all tests pass
2. Run single task: `just eval-task ruff-005 --approaches with-bobbin`
3. Check result JSON has `tool_use_summary` with non-zero `total_tool_calls` and populated `by_tool`
4. Check `.stream.jsonl` file saved alongside result JSON
5. Generate report: `bobbin-eval report results/` — verify Tool Calls, First Edit Turn, Bobbin Commands rows appear
6. Run without bobbin: `just eval-task ruff-005 --approaches no-bobbin` — verify `bobbin_commands` is empty
7. Backward compat: `bobbin-eval report results/` on old runs still works (no tool_use columns, no errors)
