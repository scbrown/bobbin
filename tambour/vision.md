# Tambour Vision

> A tambour is the frame that holds fabric taut while working with beads and thread. Tambour holds your codebase steady while AI agents work on it.

## The Middleware Gap

AI coding agents are powerful but forgetful. When Claude works on your codebase, it operates in a reactive loop: you ask, it does. But agents lack **proactive awareness**:

- They don't know what task they're supposed to be working on unless told
- They can't see the broader plan or how their work fits into it
- They "drift" from the plan if not constantly reminded
- Multiple agents have no coordination mechanism

**Beads** (the git-native issue tracker) solves the *storage* problem—tasks are tracked, dependencies are modeled, state is persistent. But Beads is passive. An agent must explicitly decide to query it (`bd ready`). If the agent forgets to check the plan, it drifts. This is the **Middleware Gap**.

Tambour fills this gap. It sits between the Beads database and the AI agent, actively *pushing* task context into the agent's attention span at critical moments.

## The Vision

Tambour is a **context injection middleware** that ensures agents have the right task context without having to ask for it.

```
                    ┌─────────────────────────────────────────┐
                    │              TAMBOUR                    │
                    │      Context Injection Middleware       │
                    └─────────────────────────────────────────┘
                           │              │              │
              ┌────────────┘              │              └────────────┐
              ▼                           ▼                          ▼
        ┌───────────┐              ┌───────────┐              ┌───────────┐
        │  BEADS    │              │  BOBBIN   │              │  AGENTS   │
        │           │              │           │              │           │
        │ Task      │              │ Semantic  │              │ Claude    │
        │ Database  │              │ Indexing  │              │ Workers   │
        └───────────┘              └───────────┘              └───────────┘
```

Tambour bridges three components:

1. **Beads** - Git-native issue tracking with first-class dependency support (the task graph)
2. **Bobbin** - Semantic code indexing for intelligent context retrieval (the code knowledge)
3. **Agents** - AI coding assistants executing tasks (the workers)

### The Bobbin + Tambour Flywheel

Used together, these systems create a powerful feedback loop:

1. **Planning**: The agent uses Tambour to define the plan in Beads
2. **Context**: When the agent picks a task, Bobbin performs semantic search for relevant code
3. **Refining**: Tambour uses temporal coupling data from Bobbin to warn: *"You're modifying Payment.ts. History shows PaymentTest.ts usually changes with this file. Did you forget to update the test?"*

## Core Principles

### 1. Tambour enables workflows, it doesn't impose them

The harness is agnostic to how you organize your work. It picks the next ready task by priority—no special filtering, no hardcoded labels. If you want to focus on specific types of work, you filter. Your workflow, your rules.

The **plugin system** extends this philosophy: tambour emits lifecycle events, and users configure how to respond. Need to refresh a search index after merges? Run tests before allowing completion? Notify an external system? Configure a plugin—don't fork tambour.

### 2. Tambour is distinct from any specific project

It emerged from bobbin development but doesn't know or care about bobbin. It orchestrates agents working on beads issues—that's all. This separation ensures tambour remains generally useful.

### 3. Agents are ephemeral, context is persistent

Agents crash. Machines restart. Networks fail. Tambour treats agents as disposable workers while ensuring context survives. The Beads database persists task state; Tambour ensures that state flows to agents automatically.

### 4. Push, don't poll

Agents shouldn't have to ask for context—context should be injected. This is the fundamental difference between a passive CLI tool and active middleware.

## Architecture

### The Interceptor Pattern

Tambour operates as a daemon and set of hooks that intercepts the agent's lifecycle. It is a "Context Orchestrator."

#### The MCP Backbone

The **Model Context Protocol (MCP)** is the transport layer for Tambour.

- **Tambour as MCP Server**: Exposes tools (`get_ready_tasks`, `claim_task`, `update_status`) and resources (`task_graph`, `current_context`) to agents via MCP
- **Dynamic Resources**: A `tambour://current_context` resource dynamically aggregates:
  1. The active task from Beads
  2. Relevant code context from Bobbin
  3. Blocking dependencies and their status

#### Hook-Based Triggers

Tambour uses Git hooks for synchronization:

- `post-checkout`: Re-hydrate context when switching branches, push "Context Shift" event
- `pre-push`: Verify that in-progress tasks linked to commits are marked complete (quality gate)
- `post-commit`: Update task graph, trigger related file warnings

### Plugin System

Tambour emits **lifecycle events** that plugins can respond to. This is the primary extension mechanism—allowing users to customize behavior without modifying tambour itself.

#### Event Types

| Event | Trigger | Example Use Cases |
|-------|---------|-------------------|
| `agent.spawned` | Agent starts in worktree | Audit logging, dashboard notification, pre-warm caches |
| `agent.finished` | Agent completes work | Run tests, lint checks, validation gates |
| `branch.merged` | Work merged to main | Refresh search index, sync external trackers, trigger CI |
| `task.claimed` | Issue → in_progress | Start time tracking, reserve resources, notify team |
| `task.completed` | Issue closed | Collect metrics, trigger dependent workflows, cleanup |
| `health.zombie` | Zombie task detected | Alert on-call, auto-recover, incident logging |

#### Configuration

Plugins are configured per-repository in `.tambour/config.toml`:

```toml
[plugins.refresh-bobbin]
on = "branch.merged"
run = "bobbin index --incremental"
blocking = false

[plugins.validate-tests]
on = "agent.finished"
run = "cargo test"
blocking = true  # Must pass before merge proceeds

[plugins.notify-complete]
on = "task.completed"
run = "curl -X POST https://hooks.example.com/task-done"
blocking = false
```

#### Blocking vs. Non-Blocking

- **Blocking plugins** (`blocking = true`): Must succeed for the workflow to proceed. Use for validation gates like tests, linting, or security checks.
- **Non-blocking plugins** (`blocking = false`): Fire-and-forget. Use for notifications, metrics, index refreshes, or external syncs.

#### Event Payload

Each event provides context to the plugin via environment variables:

```bash
# Core identification (all events)
TAMBOUR_EVENT="branch.merged"
TAMBOUR_ISSUE_ID="proj-a1b"
TAMBOUR_ISSUE_TITLE="Add user authentication"
TAMBOUR_BRANCH="proj-a1b"

# Location context
TAMBOUR_WORKTREE="/path/to/worktrees/proj-a1b"
TAMBOUR_MAIN_REPO="/path/to/main-repo"

# Change context (merge events)
TAMBOUR_MERGE_COMMIT="abc123def"
TAMBOUR_FILES_CHANGED="src/auth.rs,src/auth_test.rs"

# Agent context (agent events)
TAMBOUR_AGENT_PID="12345"
TAMBOUR_AGENT_EXIT_CODE="0"
```

The payload is intentionally minimal but sufficient to:
- Identify what changed (issue, branch, commit)
- Locate the change (worktree path, main repo path)
- Enable syncing back to the main branch

Additional context can be queried by plugins via `bd show $TAMBOUR_ISSUE_ID` or git commands as needed.

#### Future: Global Plugins

Per-repo plugins (`.tambour/config.toml`) are the current scope. Global plugins (`~/.config/tambour/plugins.toml`) are a future consideration for:
- Organization-wide policies
- Personal automation preferences
- Shared notification configurations

This requires additional work around precedence, overrides, and sync—deferred until the per-repo model is proven.

### Context Injection Strategy

Tambour creates a "System Prompt Overlay" that acts as the agent's subconscious:

- **The Ready Front**: The leading edge of the task graph—unblocked, actionable tasks. Tambour injects: *"You are working on Epic E-1. The current unblocked task is T-45 (Fix Login Handler). T-45 was blocked by T-42, completed in commit a1b2c3."*

- **Cycle Detection**: If an agent creates a circular dependency, Tambour detects it and intervenes, preventing deadlock loops.

- **Temporal Coupling Warnings**: When modifying files, Tambour warns about historically co-changed files that might need updates.

### Worktree Isolation (Multi-Agent)

For parallel agent execution, each agent gets its own git worktree:

```
main-repo/
├── .beads/          # Shared issue database
├── .tambour/        # Tambour config and plugins
├── src/
└── ...

../worktrees/
├── issue-a1b/       # Agent 1's isolated workspace
│   ├── .beads/      # Redirect to main
│   └── src/
├── issue-c2d/       # Agent 2's isolated workspace
└── issue-e3f/       # Agent 3's isolated workspace
```

This provides:
- **Filesystem isolation** - No file conflicts between agents
- **Branch per task** - Clean git history, easy merges
- **Shared beads state** - All agents see the same issue database via redirect

## Unique Value Proposition

**Tambour is the only middleware specifically designed to "drive" an agent using a structured, git-backed task graph, solving the "Lazy Agent" problem.**

Other tools treat agents as passive responders. Tambour treats them as workers that need active supervision—context injection, task assignment, progress tracking, and coordination.

| Aspect | Without Tambour | With Tambour |
|--------|-----------------|--------------|
| Task awareness | Agent must ask | Context injected automatically |
| Multi-agent | Conflicts, races | Isolated worktrees, atomic claiming |
| Agent crashes | Work lost/stuck | Automatic recovery, health monitoring |
| Plan drift | Agent forgets goals | Continuous context reinforcement |
| Dependency tracking | Manual checking | Automatic Ready Front calculation |
| Workflow extension | Fork the tool | Configure plugins |

## Current State vs. Target Architecture

### Current Implementation (Phase 1: "The Shuttle")

Shell scripts providing worktree-based isolation:

- `start-agent.sh` - Spawn agent in isolated worktree with task context
- `finish-agent.sh` - Merge branch, cleanup worktree, close issue
- `health-check.sh` - Detect and recover zombied tasks
- Justfile recipes for ergonomic CLI (`just tambour agent`)

This solves the **multi-agent coordination** problem but context injection is still manual (injected once at spawn time).

### Target Architecture (Phase 2: "The Needle")

A daemon + MCP server for continuous context injection:

- **Tambour Daemon**: Background process watching Beads state
- **MCP Server**: Exposes tools and dynamic resources to agents
- **Plugin System**: Event-driven extensibility via configuration
- **Git Hooks**: Automatic triggers for context updates
- **Bobbin Integration**: Semantic code context based on current task

This solves the **Middleware Gap**—agents receive context automatically throughout their lifecycle, not just at spawn.

### Future (Phase 3: "The Weave")

Full integration with temporal awareness:

- **Agent Pools**: Configurable concurrency, automatic respawning
- **Global Plugins**: Organization-wide policies and automation
- **Speculative Execution**: Start dependent tasks before blockers complete
- **Learning**: Track completion patterns, predict difficulty
- **Cross-Repo Orchestration**: Agents working across multiple repositories

## Non-Goals

Things tambour explicitly does not aim to do:

- **Replace CI/CD** - Tambour is for development, not deployment
- **Manage infrastructure** - No containers, no VMs, local-first
- **Lock-in to Claude** - Works with any agent that can consume MCP
- **Enterprise features** - No multi-tenant, no auth, no billing—keep it simple
- **Prescribe workflows** - Provide primitives, not opinions

## Success Metrics

Tambour succeeds when:

1. Agents never "forget" what task they're working on
2. Running 10 agents is as easy as running 1
3. Crashed agents never leave orphaned work
4. Merge conflicts from parallel work approach zero
5. Context is always relevant to the current task
6. New workflow requirements are met by configuration, not code changes
