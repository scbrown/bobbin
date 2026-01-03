#!/bin/bash
# Health check for potentially zombied beads tasks
#
# Identifies in_progress issues that may be abandoned:
# - No corresponding worktree exists
# - Worktree exists but no Claude process running in it
#
# Usage: ./scripts/health-check.sh [--fix]
#   --fix: Automatically unclaim zombied tasks

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKTREE_BASE="${REPO_ROOT}/../bobbin-worktrees"
FIX_MODE=false

if [ "$1" = "--fix" ]; then
    FIX_MODE=true
fi

# Event emission helper
emit_event() {
    local event_type="$1"
    local issue_id="$2"
    local worktree="$3"
    shift 3
    local extra_args=("$@")

    if command -v python3 &> /dev/null; then
        PYTHONPATH="$REPO_ROOT/tambour/src" python3 -m tambour events emit "$event_type" \
            ${issue_id:+--issue "$issue_id"} \
            ${worktree:+--worktree "$worktree"} \
            "${extra_args[@]}" \
            2>/dev/null || true
    fi
}

cd "$REPO_ROOT"

echo "=== Tambour Health Check ==="
echo ""

# Get all in_progress issues
IN_PROGRESS=$(bd list --status in_progress --json 2>/dev/null || echo "[]")
COUNT=$(echo "$IN_PROGRESS" | jq 'length')

if [ "$COUNT" = "0" ]; then
    echo "No in_progress issues found."
    exit 0
fi

echo "Found $COUNT in_progress issue(s):"
echo ""

ZOMBIES=()

echo "$IN_PROGRESS" | jq -r '.[] | "\(.id)\t\(.title)"' | while IFS=$'\t' read -r ISSUE_ID TITLE; do
    WORKTREE_PATH="${WORKTREE_BASE}/${ISSUE_ID}"
    STATUS="unknown"

    if [ ! -d "$WORKTREE_PATH" ]; then
        STATUS="ZOMBIE (no worktree)"
    else
        # Check if any claude process has this worktree as cwd
        # Look for claude processes whose cwd contains this worktree path
        ABSOLUTE_WT="$(cd "$WORKTREE_PATH" 2>/dev/null && pwd)" || ABSOLUTE_WT=""

        if [ -n "$ABSOLUTE_WT" ]; then
            # Find claude processes and check their working directories
            FOUND_PROCESS=false
            for pid in $(pgrep -f "claude" 2>/dev/null || true); do
                # Get the cwd of the process (macOS compatible)
                PROC_CWD=$(lsof -p "$pid" 2>/dev/null | grep cwd | awk '{print $NF}' | head -1)
                if [ "$PROC_CWD" = "$ABSOLUTE_WT" ]; then
                    FOUND_PROCESS=true
                    break
                fi
            done

            if [ "$FOUND_PROCESS" = true ]; then
                STATUS="OK (agent running)"
            else
                STATUS="ZOMBIE (no agent process)"
            fi
        else
            STATUS="ZOMBIE (worktree inaccessible)"
        fi
    fi

    echo "  $ISSUE_ID: $TITLE"
    echo "    Status: $STATUS"

    if [[ "$STATUS" == ZOMBIE* ]]; then
        # Emit health.zombie event
        emit_event "health.zombie" "$ISSUE_ID" "$WORKTREE_PATH" "--extra" "zombie_reason=$STATUS"

        if [ "$FIX_MODE" = true ]; then
            echo "    Action: Unclaiming..."
            bd update "$ISSUE_ID" --status open --assignee "" 2>/dev/null && echo "    Done." || echo "    Failed to unclaim."
        else
            echo "    Action: Run with --fix to unclaim"
        fi
    fi
    echo ""
done

echo "=== Health check complete ==="
