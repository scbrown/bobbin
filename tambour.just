# Tambour: Agent harness for beads
# These recipes will eventually move to their own project

# Default - list tambour recipes
[private]
default:
    @just --list tambour

# Spawn an agent on the next ready task
agent:
    ./scripts/start-agent.sh

# Spawn an agent on a specific issue
agent-for issue:
    ./scripts/start-agent.sh {{issue}}

# Spawn an agent filtered by label (e.g., just tambour agent-label tambour)
agent-label label:
    ./scripts/start-agent.sh --label {{label}}

# Finish and merge an agent's work (prompts to continue to next task)
finish issue:
    ./scripts/finish-agent.sh {{issue}} --merge

# Finish and merge without continuation prompt
finish-no-continue issue:
    ./scripts/finish-agent.sh {{issue}} --merge --no-continue

# Finish without merging (keep worktree)
finish-keep issue:
    ./scripts/finish-agent.sh {{issue}}

# Abort/cancel an agent (unclaim, remove worktree, delete branch)
abort issue:
    #!/usr/bin/env bash
    set -e
    WORKTREE="../bobbin-worktrees/{{issue}}"
    echo "Aborting {{issue}}..."
    bd update "{{issue}}" --status open --assignee "" || true
    bd worktree remove "$WORKTREE" --force 2>/dev/null || true
    git branch -D "{{issue}}" 2>/dev/null || true
    echo "Done."

# Check for zombied tasks (in_progress but no agent)
health:
    ./scripts/health-check.sh

# Auto-fix zombied tasks (unclaim them)
health-fix:
    ./scripts/health-check.sh --fix

# List all active worktrees
worktrees:
    bd worktree list

# Show ready tasks
ready:
    bd ready

# Show all in-progress tasks
wip:
    bd list --status in_progress

# Show issue details
show issue:
    bd show {{issue}}
