# Tambour Metrics: Tool Use Analytics for Context Intelligence

> **Status:** Design Phase
> **Epic:** bobbin-6nh
> **Labels:** tambour
> **Depends on:** bobbin-iac (Plugin System & Daemon)

## Overview

Tambour Metrics captures tool use data from Claude Code sessions to drive intelligent context injection and workflow optimization. By analyzing patterns in how agents interact with files and tools, tambour can proactively provide better context and identify files that need attention.

## Problem Statement

Currently, tambour orchestrates agents without knowledge of their behavior patterns:

1. **No context awareness** - Each session starts fresh; if agents repeatedly read the same file, we don't know
2. **No file importance signals** - Some files are read in every session, suggesting they should be pre-loaded
3. **No complexity detection** - Files that consistently cause long read times or multiple re-reads may need documentation
4. **No success/failure tracking** - We don't know which operations succeed or fail across sessions

## Solution: Plugin-Based Metrics Collection

This system integrates with tambour's existing plugin infrastructure (from bobbin-iac) rather than operating as a standalone hook. Claude Code's `PostToolUse` hook acts as a thin bridge that emits tambour events, which are then processed by the metrics collector plugin.

### Why Use the Plugin System?

1. **Unified architecture** - All tambour functionality flows through the same event/plugin system
2. **Reuse infrastructure** - Leverages existing event dispatcher, configuration, timeouts, error handling
3. **Consistency** - Metrics collector is configured the same way as other plugins (e.g., bobbin-refresh)
4. **Extensibility** - Other plugins can subscribe to `tool.*` events for their own purposes

### Data Captured

- **Tool name** - Which tool was invoked (Read, Write, Edit, Bash, etc.)
- **Tool input** - Parameters including file paths
- **Tool response** - Success/failure status and results
- **Session context** - Session ID, timestamp, worktree/issue info

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Claude Code Session                         │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐      │
│  │  Read   │    │  Write  │    │  Edit   │    │  Bash   │      │
│  └────┬────┘    └────┬────┘    └────┬────┘    └────┬────┘      │
│       │              │              │              │            │
│       └──────────────┴──────────────┴──────────────┘            │
│                              │                                   │
│                    ┌─────────▼─────────┐                        │
│                    │   PostToolUse     │                        │
│                    │   Claude Hook     │                        │
│                    └─────────┬─────────┘                        │
└──────────────────────────────┼──────────────────────────────────┘
                               │ Emits tambour event
                               │
                      ┌────────▼────────┐
                      │  tambour events │
                      │  emit tool.used │
                      └────────┬────────┘
                               │
                      ┌────────▼────────┐
                      │  Tambour Event  │
                      │  Dispatcher     │
                      └────────┬────────┘
                               │ Routes to plugins
                               │
              ┌────────────────┼────────────────┐
              │                │                │
     ┌────────▼────────┐ ┌────▼────────┐ ┌─────▼─────────┐
     │ metrics-collector│ │ (future)    │ │ (future)      │
     │ plugin           │ │ plugins     │ │ plugins       │
     └────────┬─────────┘ └─────────────┘ └───────────────┘
              │
              │ JSONL append
     ┌────────▼────────┐
     │ .tambour/       │
     │ metrics.jsonl   │
     └────────┬────────┘
              │
              │ Analyzed by
     ┌────────▼────────────────────────────────┐
     │  Aggregation / Hot Files / Complexity   │
     └─────────────────────────────────────────┘
```

### Event Flow

1. **Claude Code** executes a tool (Read, Write, Edit, etc.)
2. **PostToolUse hook** fires with tool details via stdin
3. **Bridge script** parses JSON and calls `python -m tambour events emit tool.used --data '...'`
4. **Tambour event dispatcher** routes `tool.used` event to subscribed plugins
5. **Metrics collector plugin** receives event and appends to `metrics.jsonl`

### New Event Types

| Event | Trigger | Data |
|-------|---------|------|
| `tool.used` | Any tool completes | tool_name, tool_input, tool_response, session_id |
| `tool.failed` | Tool returns error | Same as above + error details |
| `session.file_read` | Read tool completes | file_path, lines, success |
| `session.file_written` | Write/Edit completes | file_path, success |

## Data Model

### Metric Event Schema

```json
{
  "timestamp": "2026-01-05T10:30:00Z",
  "session_id": "abc123",
  "issue_id": "bobbin-xyz",
  "worktree": "/path/to/worktree",
  "tool": "Read",
  "input": {
    "file_path": "/path/to/src/main.rs",
    "offset": null,
    "limit": null
  },
  "output": {
    "success": true,
    "lines_read": 150,
    "truncated": false
  },
  "duration_ms": 45,
  "error": null
}
```

### Tool-Specific Input Fields

| Tool | Key Fields |
|------|------------|
| Read | `file_path`, `offset`, `limit` |
| Write | `file_path`, content length |
| Edit | `file_path`, `old_string` length, `new_string` length |
| Glob | `pattern`, `path` |
| Grep | `pattern`, `path`, `output_mode` |
| Bash | `command` (first token for categorization), `description` |
| WebFetch | `url`, `prompt` |
| WebSearch | `query` |
| Task | `subagent_type`, `description` |

### Aggregated Metrics (Derived)

```json
{
  "file_path": "/path/to/src/main.rs",
  "stats": {
    "total_reads": 47,
    "unique_sessions": 12,
    "avg_read_duration_ms": 52,
    "max_read_duration_ms": 340,
    "total_edits": 8,
    "edit_success_rate": 0.875,
    "last_accessed": "2026-01-05T10:30:00Z",
    "first_accessed": "2026-01-01T08:00:00Z"
  }
}
```

## Use Cases

### 1. Hot File Detection & Auto-Injection

**Problem:** Agents repeatedly read the same files (e.g., `CLAUDE.md`, `Cargo.toml`, key modules).

**Solution:** Track read frequency across sessions. Files exceeding a threshold become "hot files" and are automatically injected as context in new sessions.

```python
# Pseudo-code for hot file detection
def get_hot_files(metrics, threshold=5, window_days=7):
    recent = filter_by_time(metrics, days=window_days)
    file_counts = count_reads_by_file(recent)
    return [f for f, count in file_counts if count >= threshold]
```

**Integration Point:** `SessionStart` hook injects hot file context.

### 2. Complexity Detection & Doc Review

**Problem:** Some files consistently require:
- Multiple re-reads in a session
- Long read times (suggesting manual scrolling/exploration)
- Frequent failed edits (suggesting unclear structure)

**Solution:** Flag files with concerning patterns and optionally create documentation review tasks.

```python
# Complexity signals
signals = {
    "multiple_reads": session_reads > 3,
    "long_read_time": avg_duration_ms > 200,
    "edit_failures": edit_failure_rate > 0.3,
    "high_churn": edits_per_session > 5
}
```

**Output:** Generate beads issues for documentation improvements:
```
bobbin-auto: Review documentation for src/complex_module.rs
  - Read 15 times across 8 sessions
  - Average 3.2 re-reads per session
  - 4 failed edit attempts
```

### 3. Tool Success/Failure Tracking

**Problem:** No visibility into what's working and what's failing.

**Solution:** Track success rates by tool type and generate reports.

```
=== Tool Success Rates (Last 7 Days) ===
Read:      98.5% (1247/1266)
Write:     99.1% (432/436)
Edit:      94.2% (389/413)   ← Investigate
Bash:      87.3% (621/711)   ← Common failures
Grep:      99.8% (892/894)
```

### 4. Session Analytics

**Problem:** No insight into session productivity patterns.

**Solution:** Aggregate metrics per session/issue to understand:
- How many tools used per session
- Time spent on different file types
- Success rate per issue complexity

### 5. Future: Predictive Context Loading

With enough historical data:
- Predict which files an issue will need based on similar past issues
- Pre-load relevant context before agent starts
- Suggest related files based on co-access patterns

## Implementation Phases

### Phase 0: Prerequisites

**Dependency:** bobbin-iac (Plugin System & Daemon) must be complete.

Required infrastructure:
- Tambour event dispatcher (bobbin-sev) ✓
- Plugin configuration parser (bobbin-ac2) ✓
- Event emission from shell scripts (bobbin-ec0) ✓

### Phase 1: Event Bridge & Collection (MVP)

**Goal:** Bridge Claude Code hooks into tambour's event system and capture metrics.

1. Add new event types (`tool.used`, `tool.failed`, `session.started`) to tambour
2. Create Claude Code hook bridge script (thin layer that emits tambour events)
3. Implement `metrics-collector` plugin
4. Store events in `.tambour/metrics.jsonl`
5. Basic validation and error handling

### Phase 2: Aggregation & Queries

**Goal:** Make metrics queryable and useful.

1. Implement aggregation pipeline (per-file, per-session stats)
2. Add CLI commands: `tambour metrics show`, `tambour metrics hot-files`
3. Create periodic aggregation (avoid re-computing on every query)
4. Add time-window filtering

### Phase 3: Context Integration

**Goal:** Use metrics to improve agent sessions.

1. Hot file detection algorithm
2. Implement `context-injector` plugin (subscribes to `session.started`)
3. Configuration for thresholds and behavior
4. Testing with real workflows

### Phase 4: Intelligence & Automation

**Goal:** Proactive insights and automated actions.

1. Complexity detection heuristics
2. Automatic issue creation for problematic files
3. Dashboard/reporting interface
4. Predictive features

## Configuration

### Plugin Configuration

The metrics collector is configured as a tambour plugin in `.tambour/config.toml`:

```toml
# .tambour/config.toml

# Metrics collector plugin - subscribes to tool.* events
[plugins.metrics-collector]
on = ["tool.used", "tool.failed"]
run = "python -m tambour.metrics collect"
blocking = false
timeout = 5

# Context injector plugin - runs on session start
[plugins.context-injector]
on = "session.started"
run = "python -m tambour.metrics inject-context"
blocking = false
timeout = 10

# Metrics settings
[metrics]
enabled = true
storage_path = ".tambour/metrics.jsonl"
retention_days = 30

[metrics.capture]
# Which tools to capture metrics for
tools = ["Read", "Write", "Edit", "Bash", "Glob", "Grep"]
# Capture file paths (can be disabled for privacy)
capture_paths = true
# Capture command summaries for Bash
capture_bash_commands = true

[metrics.hot_files]
# Minimum reads to be considered "hot"
threshold = 5
# Time window for analysis
window_days = 7
# Maximum hot files to inject
max_inject = 10

[metrics.complexity]
# Enable complexity detection
enabled = true
# Re-reads threshold
multiple_read_threshold = 3
# Create issues automatically
auto_create_issues = false
```

### Claude Code Hook Configuration

A thin bridge hook in `.claude/settings.json` emits tambour events:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Read|Write|Edit|Glob|Grep|Bash",
        "hooks": [
          {
            "type": "command",
            "command": "python -m tambour events emit tool.used",
            "timeout": 5
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "python -m tambour events emit session.started",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

The bridge script:
1. Receives JSON from Claude Code via stdin
2. Parses and extracts relevant fields
3. Calls `tambour events emit <event-type>` with the data
4. Tambour's event dispatcher routes to subscribed plugins

## Privacy & Security Considerations

1. **File paths** - May contain sensitive project structure; make capture optional
2. **File contents** - Never captured; only metadata and paths
3. **Bash commands** - Capture first token only or full command (configurable)
4. **Storage** - Local only; `.tambour/` already in `.gitignore`
5. **Retention** - Automatic cleanup of old metrics (configurable)

## Success Criteria

### Phase 1 (MVP)
- [ ] Metrics collector captures PostToolUse events reliably
- [ ] Events stored in JSONL format with required fields
- [ ] No impact on agent session performance (< 5ms overhead)
- [ ] Handles errors gracefully (hook failures don't block tools)

### Phase 2
- [ ] `tambour metrics` CLI provides useful queries
- [ ] Aggregations computed efficiently
- [ ] Hot files identified correctly based on thresholds

### Phase 3
- [ ] Hot files injected into sessions via SessionStart hook
- [ ] Agents demonstrably benefit from pre-loaded context
- [ ] Configuration allows tuning behavior

### Phase 4
- [ ] Complex files flagged with actionable insights
- [ ] Automated issue creation works (when enabled)
- [ ] Metrics drive measurable productivity improvements

## Dependencies

- **bobbin-iac** - Plugin system and event infrastructure (REQUIRED)
  - Event dispatcher (bobbin-sev)
  - Plugin configuration parser (bobbin-ac2)
  - Event emission points (bobbin-ec0)
- **Claude Code hooks** - PostToolUse, SessionStart events (bridge layer)
- **beads** - For automated issue creation (Phase 4)

## Related Work

- **bobbin-iac** - Parent epic establishing plugin system and event infrastructure
- **bobbin-ysn** - Reference plugin implementation (bobbin index refresh)
- **bobbin-sev** - Event dispatcher that routes events to plugins
- **Claude Code hooks docs** - https://docs.anthropic.com/en/docs/claude-code/hooks

## Open Questions

1. **Storage format** - JSONL is simple but grows unbounded. Consider SQLite for larger deployments?
2. **Cross-repo metrics** - When tambour manages multiple repos, should metrics be per-repo or global?
3. **Metric sampling** - For very active sessions, should we sample or capture everything?
4. **Privacy defaults** - Should path capture be on or off by default?

## Appendix: Example Metric Events

### Read Event
```json
{
  "timestamp": "2026-01-05T10:30:00.123Z",
  "session_id": "sess_abc123",
  "issue_id": "bobbin-xyz",
  "tool": "Read",
  "input": {"file_path": "/Users/dev/project/src/main.rs"},
  "output": {"success": true, "lines": 247},
  "duration_ms": 38
}
```

### Failed Edit Event
```json
{
  "timestamp": "2026-01-05T10:30:05.456Z",
  "session_id": "sess_abc123",
  "issue_id": "bobbin-xyz",
  "tool": "Edit",
  "input": {
    "file_path": "/Users/dev/project/src/parser.rs",
    "old_string_len": 45,
    "new_string_len": 52
  },
  "output": {"success": false, "error": "old_string not found"},
  "duration_ms": 12
}
```

### Bash Event
```json
{
  "timestamp": "2026-01-05T10:30:10.789Z",
  "session_id": "sess_abc123",
  "issue_id": "bobbin-xyz",
  "tool": "Bash",
  "input": {
    "command_prefix": "cargo",
    "description": "Run tests"
  },
  "output": {"success": true, "exit_code": 0},
  "duration_ms": 4523
}
```
