# Tambour Product Requirements Document

## Overview

Tambour is a context injection middleware for AI coding agents. It bridges Beads (task tracking), Bobbin (code indexing), and AI agents (Claude, etc.) to enable reliable parallel development with automatic context management.

See [vision.md](./vision.md) for background on the Middleware Gap and architectural philosophy.

## Problem Statement

### Primary Problems

1. **Lazy Agent Problem**: Agents drift from plans because they must actively query for task context. If they forget, they wander.

2. **Multi-Agent Conflicts**: Running multiple agents on the same codebase causes file conflicts, race conditions when claiming tasks, and orphaned work when agents crash.

3. **No Extensibility**: Current workflow automation requires forking tools or writing wrapper scripts with no standard extension points.

### User Stories

- *As a developer*, I want to spawn multiple agents working in parallel without merge conflicts.
- *As a developer*, I want agents to automatically know what task they're working on without manual prompting.
- *As a developer*, I want crashed agents to release their claimed tasks automatically.
- *As a developer*, I want to run custom automation (tests, index refresh, notifications) at specific points in the agent lifecycle.
- *As a team lead*, I want visibility into what agents are working on and their progress.

## Phases

### Phase 1: "The Shuttle" (Current)

**Status**: Implemented

Shell scripts providing worktree isolation and basic lifecycle management.

#### Features

| Feature | Status | Description |
|---------|--------|-------------|
| Worktree creation | ✅ | Isolated workspace per agent via `bd worktree create` |
| Task claiming | ✅ | Atomic claim at spawn, prevents race conditions |
| Context injection | ✅ | Task details injected as initial prompt |
| Crash recovery | ✅ | Exit trap unclaims task on script/agent failure |
| Health monitoring | ✅ | Detect zombied tasks (in_progress, no agent) |
| Label filtering | ✅ | `--label` flag to focus on specific task types |

#### Components

- `scripts/start-agent.sh` - Agent spawner
- `scripts/finish-agent.sh` - Merge and cleanup
- `scripts/health-check.sh` - Zombie detection
- `tambour.just` - CLI recipes

#### Limitations

- Context injected once at spawn (no continuous injection)
- No plugin system
- No daemon (polling-based health checks)
- No MCP integration

---

### Phase 2: "The Needle" (Next)

**Goal**: Event-driven plugin system and daemon for continuous operation.

#### Features

##### 2.1 Plugin System

**Priority**: High

Configuration-driven event hooks allowing custom automation without modifying tambour.

**Requirements**:

- [ ] Parse `.tambour/config.toml` for plugin definitions
- [ ] Emit lifecycle events at appropriate points
- [ ] Execute plugins with event payload as environment variables
- [ ] Support blocking (must succeed) and non-blocking (fire-and-forget) modes
- [ ] Log plugin execution results
- [ ] Timeout handling for blocking plugins

**Events**:

| Event | Trigger Point | Payload |
|-------|---------------|---------|
| `agent.spawned` | After worktree created, before Claude starts | issue_id, worktree, branch |
| `agent.finished` | After Claude exits (any exit code) | issue_id, worktree, exit_code |
| `branch.merged` | After successful merge to main | issue_id, branch, merge_commit, files_changed |
| `task.claimed` | After `bd update --claim` succeeds | issue_id, assignee |
| `task.completed` | After `bd close` succeeds | issue_id, resolution |
| `health.zombie` | When zombie detected | issue_id, last_seen, worktree_exists |

**Configuration Schema**:

```toml
[tambour]
version = "1"

[plugins.example-plugin]
on = "branch.merged"           # Required: event to trigger on
run = "bobbin index"           # Required: command to execute
blocking = false               # Optional: default false
timeout = 60                   # Optional: seconds, default 30
enabled = true                 # Optional: default true
```

**Environment Variables** (passed to all plugins):

```bash
# Event identification
TAMBOUR_EVENT="branch.merged"
TAMBOUR_TIMESTAMP="2024-01-15T10:30:00Z"

# Issue context
TAMBOUR_ISSUE_ID="proj-a1b"
TAMBOUR_ISSUE_TITLE="Add user authentication"
TAMBOUR_ISSUE_TYPE="task"
TAMBOUR_BRANCH="proj-a1b"

# Location context
TAMBOUR_WORKTREE="/absolute/path/to/worktrees/proj-a1b"
TAMBOUR_MAIN_REPO="/absolute/path/to/main-repo"
TAMBOUR_BEADS_DB="/absolute/path/to/main-repo/.beads"

# Event-specific (examples)
TAMBOUR_MERGE_COMMIT="abc123"        # branch.merged
TAMBOUR_FILES_CHANGED="a.rs,b.rs"    # branch.merged
TAMBOUR_EXIT_CODE="0"                # agent.finished
TAMBOUR_AGENT_PID="12345"            # agent.spawned, agent.finished
```

##### 2.2 Tambour Daemon

**Priority**: Medium

Background process for health monitoring and event dispatch.

**Requirements**:

- [ ] Daemonize with PID file (`~/.tambour/daemon.pid`)
- [ ] Watch for worktree changes (creation, removal)
- [ ] Periodic health checks (configurable interval)
- [ ] Event dispatch to plugins
- [ ] Graceful shutdown on SIGTERM
- [ ] Log rotation

**CLI**:

```bash
tambour daemon start          # Start daemon
tambour daemon stop           # Stop daemon
tambour daemon status         # Show daemon status
tambour daemon logs           # Tail daemon logs
```

##### 2.3 Improved Health Monitoring

**Priority**: Medium

Proactive health checks with configurable recovery.

**Requirements**:

- [ ] Heartbeat detection (agent writes timestamp periodically)
- [ ] Configurable zombie threshold (default: 5 minutes no heartbeat)
- [ ] Auto-recovery option (unclaim zombies automatically)
- [ ] Emit `health.zombie` event for plugin handling

##### 2.4 CLI Improvements

**Priority**: Low

Better ergonomics and discoverability.

**Requirements**:

- [ ] `tambour status` - Dashboard view of all agents and tasks
- [ ] `tambour config` - Validate and show effective configuration
- [ ] `tambour events` - List recent events (from log)
- [ ] Color output and progress indicators

##### 2.5 Task Depletion & Context Continuity

**Priority**: Low

Maintain workflow momentum when tasks complete.

**Requirements**:

- [ ] Detect when ready queue is empty after task completion
- [ ] Present completion summary showing:
  - The completed task
  - Any epics auto-closed (when all children complete)
  - Any tasks closed via `closes:` references
- [ ] Prompt user to create more tasks when queue is depleted
- [ ] If user creates tasks, spawn new agent session with context injection
- [ ] Context injection includes prior completion info so new agent has continuity

**Flow**:

1. `finish-agent.sh` completes → task closed
2. Query `bd ready --json` for remaining tasks
3. If empty:
   - Show completion summary
   - Prompt "Create more tasks? (y/n)"
   - If yes, open interactive task creation
   - After creation, spawn new agent with completion context
4. If tasks remain:
   - Prompt "Continue to next task? (y/n)"
   - If yes, spawn with completion context injected

**Context Injection Format**:

```
Previous session completed:
- Task: bobbin-xyz "Implement feature X"
- Also closed: bobbin-abc (via closes: reference)
- Epic completed: bobbin-epic "Phase 1" (all children done)

You are now assigned to: bobbin-new "Next task"
```

---

### Phase 3: "The Weave" (Future)

**Goal**: MCP integration and advanced orchestration.

#### Features (Planned)

##### 3.1 MCP Server

Expose tambour functionality via Model Context Protocol.

**Tools**:
- `get_ready_tasks` - Return unblocked tasks
- `claim_task` - Atomically claim a task
- `update_task` - Update task status/progress
- `get_context` - Get current task context

**Resources**:
- `tambour://task/{id}` - Task details
- `tambour://ready` - Ready task list
- `tambour://context` - Dynamic context for current task

##### 3.2 Bobbin Integration

Semantic code context based on current task.

- Pre-index worktrees for faster agent startup
- Task-aware search (scope results to relevant code)
- Temporal coupling warnings during edits

##### 3.3 Agent Pools

Managed concurrency for parallel execution.

- Configurable pool size
- Automatic respawning when agents complete
- Load balancing across available tasks
- Queue management

##### 3.4 Cross-Repo Orchestration

Agents working across multiple repositories.

- Shared daemon managing multiple repos
- Cross-repo dependency tracking
- Unified task queue

---

## Configuration Reference

### File Location

`.tambour/config.toml` in repository root.

### Full Schema

```toml
[tambour]
version = "1"

[agent]
default_cli = "claude"        # Default agent CLI command

[daemon]
health_interval = 60          # Seconds between health checks
zombie_threshold = 300        # Seconds before task is considered zombie
auto_recover = false          # Automatically unclaim zombies

[worktree]
base_path = "../{repo}-worktrees"   # Template for worktree location
# {repo} expands to repository name

[context.providers.tree]
run = "tambour/providers/tree.sh"
timeout = 10
# Arbitrary options are passed as env vars (e.g. TREE_EXCLUDE)
exclude = [".git", "node_modules", "target"]
depth = 4

[plugins.refresh-bobbin]
on = "branch.merged"
run = "bobbin index --incremental"
blocking = false
timeout = 120
enabled = true

[plugins.run-tests]
on = "agent.finished"
run = "cargo test"
blocking = true
timeout = 300

[plugins.notify-slack]
on = "task.completed"
run = "./scripts/notify-slack.sh"
blocking = false
```

---

## Technical Decisions

### Language: Shell → Python

**Phase 1**: Shell scripts. Already working, zero dependencies, easy to modify.

**Phase 2+**: Python. When we need higher abstraction (daemon, plugin system, MCP server), Python provides:
- Excellent subprocess orchestration
- Good file watching libraries (watchdog)
- Clean config parsing (tomllib in 3.11+)
- MCP SDK support for Phase 3
- Faster iteration than compiled languages

Rust is reserved for performance-critical work (bobbin). Tambour is orchestration glue—Python is the right tool.

### No External Dependencies

Tambour should work with just git, beads, and a shell. No database servers, no container runtimes, no cloud services required.

### Beads as Source of Truth

Tambour reads from and writes to Beads. It never maintains its own task state—Beads is the single source of truth.

### Git Worktrees for Isolation

Native git worktrees (via `bd worktree`) provide isolation without the overhead of full clones. Shared `.beads` via redirect file.

### Event-Driven Over Polling

Where possible, use file watchers and git hooks instead of polling. Daemon exists for cases where events aren't available (e.g., detecting crashed processes).

---

## Success Criteria

### Phase 2 Complete When

- [ ] Plugins execute on all defined events
- [ ] Blocking plugins can gate workflow progression
- [ ] Daemon runs reliably in background
- [ ] Health checks detect and optionally recover zombies automatically
- [ ] At least one real plugin (bobbin refresh) works end-to-end

### Phase 3 Complete When

- [ ] MCP server exposes core tambour functionality
- [ ] Agents can query tambour for context via MCP
- [ ] Bobbin integration provides task-aware search
- [ ] Agent pools manage concurrent execution

---

## Open Questions

1. **Plugin sandboxing**: Should plugins run in a restricted environment? Current plan: no, trust user-configured commands.

2. **Event replay**: Should tambour store events for replay/debugging? Current plan: log only, no replay.

3. **Multi-repo coordination**: How do worktrees work when a task spans repos? Deferred to Phase 3.

4. **Windows support**: Worktree paths and shell scripts assume Unix. Scope for later.

---

## Appendix: Migration Path

### From Phase 1 to Phase 2

1. Install tambour daemon alongside existing scripts
2. Create `.tambour/config.toml` with existing workflow encoded as plugins
3. Migrate from justfile recipes to `tambour` CLI
4. Deprecate shell scripts once daemon is stable

### Backwards Compatibility

Phase 1 scripts will continue to work. Phase 2 adds capabilities without breaking existing workflows.
