#!/bin/bash
# Start a Claude agent in an isolated worktree for a beads task
#
# Usage: ./scripts/start-agent.sh [issue-id]
#   If no issue-id provided, picks the first ready task (skipping epics)

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Run startup health check (warns about zombies, doesn't auto-fix)
"$REPO_ROOT/scripts/health-check.sh" 2>/dev/null | grep -q "ZOMBIE" && {
    echo "⚠️  Warning: Found zombied tasks. Run './scripts/health-check.sh --fix' to clean up."
    echo ""
}

# Track if we claimed an issue (for cleanup on failure)
CLAIMED_ISSUE=""

# Cleanup handler - unclaim if we exit before Claude takes over
cleanup() {
    local exit_code=$?
    if [ -n "$CLAIMED_ISSUE" ] && [ $exit_code -ne 0 ]; then
        echo ""
        echo "Script failed - unclaiming $CLAIMED_ISSUE..."
        bd update "$CLAIMED_ISSUE" --status open --assignee "" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Get issue ID from argument or pick next ready task
if [ -n "$1" ]; then
    ISSUE_ID="$1"
else
    # Get first ready task (not epic)
    ISSUE_ID=$(bd ready --json | jq -r '[.[] | select(.issue_type == "task")][0].id')

    if [ -z "$ISSUE_ID" ] || [ "$ISSUE_ID" = "null" ]; then
        echo "No ready tasks available"
        bd ready
        exit 1
    fi
fi

ISSUE_TITLE=$(bd show "$ISSUE_ID" --json | jq -r '.[0].title')
WORKTREE_PATH="../bobbin-worktrees/${ISSUE_ID}"

echo "=== Starting agent for: $ISSUE_ID ==="
echo "Title: $ISSUE_TITLE"
echo ""

# Create worktree if it doesn't exist (using beads native command)
if [ -d "$WORKTREE_PATH" ]; then
    echo "Worktree already exists, reusing..."
else
    echo "Creating worktree with beads redirect..."
    bd worktree create "$WORKTREE_PATH" --branch "$ISSUE_ID"
fi

# Claim the issue (sets assignee + status to in_progress)
echo "Claiming $ISSUE_ID..."
if bd update "$ISSUE_ID" --claim; then
    CLAIMED_ISSUE="$ISSUE_ID"
else
    echo "Warning: Could not claim issue (may already be claimed)"
fi

ABSOLUTE_PATH="$(cd "$WORKTREE_PATH" && pwd)"
echo ""
echo "=== Launching Claude in worktree ==="
echo "Path: $ABSOLUTE_PATH"
echo ""

# Capture the bd show output for the prompt
BD_SHOW_OUTPUT=$(bd show "$ISSUE_ID")

# Build the prompt showing what we executed
PROMPT="You have been assigned to work on a beads issue. Here's what was executed to show you the task:

\$ bd show $ISSUE_ID
$BD_SHOW_OUTPUT

You are now in a git worktree at: $ABSOLUTE_PATH
Branch: $ISSUE_ID

Please begin working on this task."

# Start Claude in the worktree with the task prompt
cd "$ABSOLUTE_PATH"
claude "$PROMPT"
CLAUDE_EXIT=$?

# Clear claimed issue so trap doesn't unclaim on normal exit
if [ $CLAUDE_EXIT -eq 0 ]; then
    CLAIMED_ISSUE=""
else
    echo ""
    echo "Claude exited with code $CLAUDE_EXIT"
fi

exit $CLAUDE_EXIT
