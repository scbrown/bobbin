# Tambour: Agent Harness for Beads

> **Note:** Tambour currently lives in the bobbin repository as it emerged from multi-agent workflow needs. It will eventually become its own module. See `AGENT.md` for details on what belongs to tambour vs bobbin.

Tambour is an agent orchestration harness that coordinates multiple AI agents working on [beads](https://github.com/steveyegge/beads) issues. The name comes from embroidery - a tambour is the frame that holds fabric taut while working with beads and thread.

## Vision

Tambour bridges three components:

1. **Beads** - Issue tracking with first-class dependency support
2. **Bobbin** - Semantic code indexing for agent context
3. **Agent Harness** - Orchestration layer for parallel agent execution

The goal is reliable multi-agent development where agents can work on independent tasks simultaneously without conflicts.

## Core Problem

When spawning multiple Claude agents to work on a codebase:
- Agents working in the same directory create merge conflicts
- Agents may grab the same task from the ready queue
- Crashed agents leave tasks in limbo (in_progress but abandoned)
- No visibility into which agent is working on what

## Solution: Worktree Isolation

Each agent gets its own git worktree, providing:
- **Filesystem isolation** - No file conflicts between agents
- **Branch per task** - Clean git history, easy merges
- **Shared beads database** - All agents see the same issue state (via beads redirect)

## Current Implementation

### Justfile Recipes

```bash
just agent              # Spawn agent on next ready task
just agent-for <issue>  # Spawn agent on specific issue
just finish <issue>     # Merge branch and cleanup
just health             # Check for zombied tasks
just health-fix         # Auto-unclaim zombied tasks
just wip                # Show in-progress tasks
just worktrees          # List active worktrees
```

### Scripts

**`scripts/start-agent.sh [issue-id]`**

Spawns an agent for a beads task:
1. Picks next ready task (or uses provided issue-id)
2. Creates worktree via `bd worktree create` (with beads redirect)
3. Claims the issue atomically (prevents race conditions)
4. Launches Claude with task context as initial prompt
5. On failure/crash, automatically unclaims the issue

**`scripts/finish-agent.sh <issue-id> [--merge]`**

Cleans up after agent completion:
1. Merges branch to main (with `--merge`)
2. Removes worktree via `bd worktree remove`
3. Closes the beads issue

**`scripts/health-check.sh [--fix]`**

Detects zombied tasks (in_progress but abandoned):
1. Finds all in_progress issues
2. Checks if worktree exists for each
3. Checks if Claude process is running in worktree
4. With `--fix`, automatically unclaims zombied tasks

### Failure Handling

The harness handles these failure modes:

| Failure | Handling |
|---------|----------|
| Script fails before Claude starts | Trap unclaims issue |
| Claude crashes (exit != 0) | Trap unclaims issue |
| Claude exits normally | Issue stays claimed |
| `kill -9` on process | `just health-fix` recovers |
| Unknown zombie | `just health` detects, `just health-fix` recovers |

### Usage

```bash
# Spawn agent on next ready task
just agent

# Spawn agent on specific task
just agent-for bobbin-j0x

# Spawn multiple agents rapidly
for i in 1 2 3; do
    just agent &
done

# Check for zombied tasks
just health

# Auto-fix zombied tasks
just health-fix

# After agent completes, merge and cleanup
just finish bobbin-j0x
```

## Future Directions

### Agent Pool Management
- Configurable number of concurrent agents
- Automatic respawning when agents complete
- Load balancing across available tasks

### Health Monitoring
- Heartbeat detection for stale agents
- Automatic recovery of abandoned tasks
- Dashboard showing agent status

### Bobbin Integration
- Pre-index worktrees for agent context
- Semantic search scoped to relevant code
- Related file suggestions based on task

### Beads Integration
- Agent comments synced to issue history
- Automatic progress updates
- Dependency-aware task assignment (don't assign blocked tasks)
