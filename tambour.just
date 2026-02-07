# Tambour: Agent harness for beads
# These recipes will eventually move to their own project

# Default - list tambour recipes
[private]
default:
    @just --list tambour

# Ensure tambour venv exists (auto-setup on first use)
[private]
ensure-venv:
    #!/usr/bin/env bash
    if [ ! -f ".venv/bin/python" ] || [ ! -f ".tambour-source" ]; then
        ./scripts/ensure-tambour-venv.sh
    fi

# Setup tambour venv and hooks
setup:
    ./scripts/ensure-tambour-venv.sh

# Run tambour Python tests
test *args: ensure-venv
    #!/usr/bin/env bash
    TAMBOUR_SRC=$(cat .tambour-source)
    .venv/bin/python -m pytest "$TAMBOUR_SRC/tests" {{ args }}

# Show metrics summary
metrics *args: ensure-venv
    .venv/bin/python -m tambour metrics {{ args }}

# Spawn an agent on the next ready task
agent cli="":
    ./scripts/start-agent.sh {{ if cli == "" { "" } else { "--agent-cli " + cli } }}

# Spawn an agent on a specific issue
agent-for issue cli="":
    ./scripts/start-agent.sh {{issue}} {{ if cli == "" { "" } else { "--agent-cli " + cli } }}

# Spawn an agent filtered by label (e.g., just tambour agent-label tambour)
agent-label label cli="":
    ./scripts/start-agent.sh --label {{label}} {{ if cli == "" { "" } else { "--agent-cli " + cli } }}

# Spawn a Gemini agent on the next ready task
gemini:
    ./scripts/start-agent.sh --agent-cli gemini

# Spawn a Gemini agent on a specific issue
gemini-for issue:
    ./scripts/start-agent.sh {{issue}} --agent-cli gemini

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
abort issue: ensure-venv
    .venv/bin/python -m tambour abort "{{issue}}"

# Cleanup a stuck task (retry finish/merge for closed/broken tasks)
cleanup issue:
    ./scripts/finish-agent.sh {{issue}} --merge --no-continue

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

# Create a spinoff issue (for discovered bugs, improvements, or subtasks)
# Usage: just tambour spinoff "Title" [--type bug] [--priority P1] [--labels label1,label2] [--blocks issue-id]
spinoff title *args:
    #!/usr/bin/env bash
    set -e

    # Parse arguments
    TYPE="task"
    PRIORITY="P2"
    LABELS=""
    BLOCKS=""
    DESCRIPTION=""

    while [[ $# -gt 0 ]]; do
        case $1 in
            --type) TYPE="$2"; shift 2 ;;
            --priority) PRIORITY="$2"; shift 2 ;;
            --labels) LABELS="$2"; shift 2 ;;
            --blocks) BLOCKS="$2"; shift 2 ;;
            --description) DESCRIPTION="$2"; shift 2 ;;
            *) shift ;;
        esac
    done

    # Build the bd create command
    CMD="bd create \"{{title}}\" --type $TYPE --priority $PRIORITY"

    if [[ -n "$LABELS" ]]; then
        CMD="$CMD --labels $LABELS"
    fi

    if [[ -n "$DESCRIPTION" ]]; then
        CMD="$CMD --description \"$DESCRIPTION\""
    fi

    # Create the issue and capture ID
    ISSUE_ID=$(eval "$CMD --silent")
    echo "Created issue: $ISSUE_ID"

    # Add blocker dependency if specified
    if [[ -n "$BLOCKS" ]]; then
        bd dep add "$BLOCKS" --blocked-by "$ISSUE_ID"
        echo "Added as blocker for: $BLOCKS"
    fi

    bd show "$ISSUE_ID"

# === Merge Lock ===
# Distributed lock using git refs to serialize merges across agents/worktrees

LOCK_REF := "refs/tambour/merge-lock"

# Check merge lock status
lock-status:
    #!/usr/bin/env bash
    set -e
    git fetch origin {{LOCK_REF}} 2>/dev/null || {
        echo "Lock: FREE (no lock held)"
        exit 0
    }
    echo "Lock: HELD"
    git cat-file -p FETCH_HEAD 2>/dev/null || echo "(no metadata)"

# Force-release the merge lock (admin/recovery)
lock-release:
    #!/usr/bin/env bash
    set -e
    echo "Force-releasing merge lock..."
    git push origin --delete {{LOCK_REF}} 2>/dev/null && echo "Lock released." || echo "Lock was not held."

# Acquire merge lock (used internally by finish)
[private]
lock-acquire holder:
    #!/usr/bin/env bash
    set -e
    LOCK_REF="{{LOCK_REF}}"
    HOLDER="{{holder}}"
    TIMEOUT=${TIMEOUT:-300}
    POLL_INTERVAL=5
    DEADLINE=$(($(date +%s) + TIMEOUT))

    # Create lock metadata
    LOCK_DATA=$(cat <<EOF
    {
      "holder": "$HOLDER",
      "acquired_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
      "host": "$(hostname)",
      "pid": $$
    }
    EOF
    )

    # Create commit object for lock
    BLOB_SHA=$(echo "$LOCK_DATA" | git hash-object -w --stdin)
    TREE_SHA=$(echo -e "100644 blob $BLOB_SHA\tlock.json" | git mktree)
    COMMIT_SHA=$(git commit-tree "$TREE_SHA" -m "merge lock: $HOLDER")

    while [ $(date +%s) -lt $DEADLINE ]; do
        if git push origin "$COMMIT_SHA:$LOCK_REF" 2>/dev/null; then
            echo "Acquired merge lock for $HOLDER"
            exit 0
        fi

        # Check who holds it
        git fetch origin "$LOCK_REF" 2>/dev/null || true
        echo "Waiting for lock... ($(git cat-file -p FETCH_HEAD 2>/dev/null | head -5 || echo 'unknown holder'))"
        sleep $POLL_INTERVAL
    done

    echo "ERROR: Timeout waiting for merge lock"
    exit 1

# Release merge lock (used internally by finish)
[private]
lock-release-for holder:
    #!/usr/bin/env bash
    set -e
    LOCK_REF="{{LOCK_REF}}"
    HOLDER="{{holder}}"

    # Verify we hold the lock
    git fetch origin "$LOCK_REF" 2>/dev/null || {
        echo "Warning: lock already released"
        exit 0
    }

    CURRENT_HOLDER=$(git cat-file -p FETCH_HEAD 2>/dev/null | grep '"holder"' | cut -d'"' -f4 || echo "")
    if [ "$CURRENT_HOLDER" != "$HOLDER" ]; then
        echo "ERROR: Lock held by '$CURRENT_HOLDER', not '$HOLDER'"
        exit 1
    fi

    git push origin --delete "$LOCK_REF" && echo "Released merge lock for $HOLDER"
