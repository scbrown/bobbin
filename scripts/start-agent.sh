#!/bin/bash
# Start a Claude agent in an isolated worktree for a beads task
#
# Usage: ./scripts/start-agent.sh [issue-id] [--label <label>]
#   issue-id: Work on specific issue
#   --label:  Filter ready tasks by label
#   If no args, picks the next ready task by priority (skipping epics)

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

# Parse arguments
ISSUE_ID=""
FILTER_LABEL=""
AGENT_CLI=""

while [ $# -gt 0 ]; do
    case "$1" in
        --label)
            FILTER_LABEL="$2"
            shift 2
            ;;
        --agent-cli)
            AGENT_CLI="$2"
            shift 2
            ;;
        *)
            ISSUE_ID="$1"
            shift
            ;;
    esac
done

# Resolve Agent CLI
if [ -z "$AGENT_CLI" ]; then
    # Try to get from config if tambour is available
    if command -v python3 &> /dev/null; then
        AGENT_CLI=$(PYTHONPATH="$REPO_ROOT/tambour/src" python3 -m tambour config get agent.default_cli 2>/dev/null) || true
    fi
fi

# Fallback to claude if still empty
if [ -z "$AGENT_CLI" ]; then
    echo "💡 Note: No agent CLI specified or configured. Defaulting to 'claude'."
    AGENT_CLI="claude"
fi

# Get issue ID from argument or pick next ready task
if [ -z "$ISSUE_ID" ]; then
    READY_JSON=$(bd ready --json)

    if [ -n "$FILTER_LABEL" ]; then
        # Filter by label if specified
        ISSUE_ID=$(echo "$READY_JSON" | jq -r --arg label "$FILTER_LABEL" '
            [.[] | select(.issue_type == "task" and ((.labels // []) | index($label)))][0].id
        ')
    else
        # Get first ready task (not epic)
        ISSUE_ID=$(echo "$READY_JSON" | jq -r '[.[] | select(.issue_type == "task")][0].id')
    fi

    if [ -z "$ISSUE_ID" ] || [ "$ISSUE_ID" = "null" ]; then
        echo "No ready tasks available${FILTER_LABEL:+ with label '$FILTER_LABEL'}"
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
echo "=== Launching $AGENT_CLI in worktree ==="
echo "Path: $ABSOLUTE_PATH"
echo ""

# Capture the bd show output for the prompt
BD_SHOW_OUTPUT=$(bd show "$ISSUE_ID")

# Build the prompt showing what we executed
# Include completion context from previous session if available
CONTEXT_PREFIX=""
if [ -n "$TAMBOUR_COMPLETION_CONTEXT" ]; then
    CONTEXT_PREFIX="$TAMBOUR_COMPLETION_CONTEXT

---

"
fi

BASE_PROMPT="${CONTEXT_PREFIX}You have been assigned to work on a beads issue. Here's what was executed to show you the task:

\$ bd show $ISSUE_ID
$BD_SHOW_OUTPUT

You are now in a git worktree at: $ABSOLUTE_PATH
Branch: $ISSUE_ID

Begin working on this task now:
1. Read CLAUDE.md and any relevant docs to understand the project
2. Explore the codebase to understand what exists and what you need to build
3. Implement the task, committing your changes as you go
4. When complete, inform the user the task is ready for review

Start immediately - do not ask for confirmation."

# Collect context from configured providers
# Context providers can use the prompt to generate task-aware context
cd "$ABSOLUTE_PATH"
INJECTED_CONTEXT=""
if command -v python3 &> /dev/null; then
    # Write prompt to temp file for provider access
    PROMPT_FILE=$(mktemp)
    echo "$BASE_PROMPT" > "$PROMPT_FILE"

    # Collect context (providers run in worktree, receive prompt via env/file)
    # Set PYTHONPATH to find tambour module
    INJECTED_CONTEXT=$(PYTHONPATH="$REPO_ROOT/tambour/src" python3 -m tambour context collect \
        --prompt "$PROMPT_FILE" \
        --issue "$ISSUE_ID" \
        --worktree "$ABSOLUTE_PATH" \
        --main-repo "$REPO_ROOT" 2>/dev/null) || true

    rm -f "$PROMPT_FILE"
fi

# Build final prompt with injected context
if [ -n "$INJECTED_CONTEXT" ]; then
    PROMPT="${BASE_PROMPT}

---

${INJECTED_CONTEXT}"
else
    PROMPT="$BASE_PROMPT"
fi

# Start Agent in the worktree with the task prompt
if [[ "$AGENT_CLI" == "gemini"* ]]; then
    $AGENT_CLI -i "$PROMPT"
else
    $AGENT_CLI "$PROMPT"
fi
AGENT_EXIT=$?

# Clear claimed issue so trap doesn't unclaim on normal exit
if [ $AGENT_EXIT -eq 0 ]; then
    CLAIMED_ISSUE=""
else
    echo ""
    echo "$AGENT_CLI exited with code $AGENT_EXIT"
fi

# Check if worktree still exists (wasn't merged during the session)
# Go back to the main repo to check
cd "$REPO_ROOT"
if [ -d "$WORKTREE_PATH" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "⚠️  Worktree still exists at: $ABSOLUTE_PATH"
    echo ""
    echo "The agent session ended but the task wasn't merged."
    echo "To finish and merge the task, run:"
    echo ""
    echo "    just tambour finish $ISSUE_ID"
    echo ""
    echo "Or to abort and discard changes:"
    echo ""
    echo "    just tambour abort $ISSUE_ID"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
fi

exit $AGENT_EXIT
