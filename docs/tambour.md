# Tambour: Agent Harness for Beads

> **Note:** Tambour currently lives in the bobbin repository as it emerged from multi-agent workflow needs. It will eventually become its own module. See `AGENT.md` for details on what belongs to tambour vs bobbin.

Tambour is an agent orchestration harness that coordinates multiple AI agents working on [beads](https://github.com/steveyegge/beads) issues. The name comes from embroidery - a tambour is the frame that holds fabric taut while working with beads and thread.

## Tenets

1. **Tambour enables workflows, it doesn't impose them.**
   The harness is agnostic to how you organize your work. It picks the next ready task by priority - no special filtering, no hardcoded labels. If you want to focus on a specific label, use `--label`. Your workflow, your rules.

2. **Tambour is distinct from any specific project.**
   It emerged from bobbin development but doesn't know or care about bobbin. It orchestrates agents working on beads issues - that's it.

3. **Tambour will eventually be extracted.**
   It lives here temporarily while the interface stabilizes. When it needs to orchestrate agents across multiple repositories, it becomes its own project.

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

Tambour is a just module - all recipes are prefixed with `tambour`:

```bash
just tambour agent                  # Spawn agent on next ready task (by priority)
just tambour agent-for <issue>      # Spawn agent on specific issue
just tambour agent-label <label>    # Spawn agent filtered by label
just tambour finish <issue>         # Merge branch, cleanup, prompt for next task
just tambour finish-no-continue <issue>  # Merge and cleanup without continuation prompt
just tambour abort <issue>          # Cancel agent (unclaim, remove worktree)
just tambour health                 # Check for zombied tasks
just tambour health-fix             # Auto-unclaim zombied tasks
just tambour wip                    # Show in-progress tasks
just tambour worktrees              # List active worktrees
just tambour ready                  # Show ready tasks
```

### Scripts

**`scripts/start-agent.sh [issue-id] [--label <label>]`**

Spawns an agent for a beads task:
1. Runs health check (warns about zombies)
2. Picks next ready task by priority (or filters by `--label`, or uses provided issue-id)
3. Creates worktree via `bd worktree create` (with beads redirect)
4. Claims the issue atomically (prevents race conditions)
5. Injects `bd show` output as initial prompt so agent sees the task details
6. Launches Claude with explicit execution instructions
7. On failure/crash, automatically unclaims the issue

**`scripts/finish-agent.sh <issue-id> [--merge] [--no-continue]`**

Cleans up after agent completion:
1. Merges branch to main (with `--merge`)
2. Removes worktree via `bd worktree remove`
3. Closes the beads issue
4. Auto-closes any epics that became eligible (all children complete)
5. Shows completion summary with closed tasks and epics
6. Prompts to continue to next task or create new tasks (unless `--no-continue`)
7. Injects completion context into next agent session

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
# Spawn agent on next ready task (by priority)
just tambour agent

# Spawn agent on specific task
just tambour agent-for bobbin-j0x

# Spawn agent filtered by label (e.g., focus on "backend" tasks)
just tambour agent-label backend

# Spawn multiple agents rapidly
for i in 1 2 3; do
    just tambour agent &
done

# Check for zombied tasks
just tambour health

# Auto-fix zombied tasks
just tambour health-fix

# Abort/cancel an agent (if started by mistake)
just tambour abort bobbin-j0x

# After agent completes, merge and cleanup
just tambour finish bobbin-j0x
```

## Current Features

### Task Depletion & Context Continuity
After completing a task, the finish script provides workflow continuity:
- **Completion summary** - Shows the task that was just closed
- **Epic awareness** - Detects and auto-closes epics when all children are complete
- **Task depletion detection** - Checks if the ready queue is empty
- **Interactive continuation** - Prompts to continue to next task or create new tasks
- **Context injection** - Passes completion context to the next agent session

Example workflow:
```bash
$ just tambour finish bobbin-xyz

=== Completion Summary ===
✓ Task: bobbin-xyz "Implement feature X"

Epics completed:
  ✓ bobbin-abc "Phase 1" (all children done)

📋 3 ready task(s) remaining
   Next: bobbin-def "Next feature"

Continue to next task? (y/n)
```

When continuing, the next agent session receives:
```
Previous session completed:
- Task: bobbin-xyz "Implement feature X"
Epics completed:
- bobbin-abc "Phase 1" (all children done)

---

You have been assigned to work on a beads issue...
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
