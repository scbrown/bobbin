#!/bin/bash
# Cleanup after agent completes work in a worktree
#
# Usage: ./scripts/finish-agent.sh <issue-id> [--merge] [--no-continue]
#   --merge: merge the branch back to main and remove worktree
#   --no-continue: skip the "continue to next task" flow after completion

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Determine if we're running from inside a worktree or the main repo
# The script could be in:
#   1. Main repo: /path/to/bobbin/scripts/finish-agent.sh
#   2. Worktree: /path/to/bobbin-worktrees/bobbin-xxx/scripts/finish-agent.sh
# We need to find the MAIN repo root, not the worktree root

# Get the git toplevel - this will be the worktree if we're in one
GIT_TOPLEVEL="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)"

# Check if this is a worktree by looking for the .git file (worktrees have a .git file, main repo has .git directory)
if [ -f "$GIT_TOPLEVEL/.git" ]; then
    # We're in a worktree - need to find the main repo
    # The .git file contains: "gitdir: /path/to/main/.git/worktrees/branch-name"
    MAIN_GIT_DIR="$(cat "$GIT_TOPLEVEL/.git" | sed 's/^gitdir: //' | sed 's|/worktrees/.*||')"
    REPO_ROOT="$(dirname "$MAIN_GIT_DIR")"
else
    # We're in the main repo
    REPO_ROOT="$GIT_TOPLEVEL"
fi

WORKTREE_BASE="${REPO_ROOT}/../bobbin-worktrees"

if [ -z "$1" ]; then
    echo "Usage: $0 <issue-id> [--merge] [--no-continue]"
    echo ""
    echo "Active worktrees:"
    bd worktree list
    exit 1
fi

ISSUE_ID="$1"
WORKTREE_PATH="${WORKTREE_BASE}/${ISSUE_ID}"
BRANCH_NAME="$ISSUE_ID"
DO_MERGE=false
NO_CONTINUE=false

# Parse remaining arguments
shift
while [ $# -gt 0 ]; do
    case "$1" in
        --merge)
            DO_MERGE=true
            shift
            ;;
        --no-continue)
            NO_CONTINUE=true
            shift
            ;;
        *)
            shift
            ;;
    esac
done

if [ ! -d "$WORKTREE_PATH" ]; then
    echo "Worktree not found: $WORKTREE_PATH"
    echo ""
    echo "Active worktrees:"
    bd worktree list
    exit 1
fi

echo "=== Finishing agent work for: $ISSUE_ID ==="

if [ "$DO_MERGE" = true ]; then
    echo "Merging $BRANCH_NAME into main..."

    # Change to main repo first - this is critical if we're running from inside a worktree
    # We cannot checkout a different branch while in the worktree we're about to remove
    cd "$REPO_ROOT"

    # Ensure we're on main
    git checkout main

    # Merge the branch
    git merge "$BRANCH_NAME" --no-edit

    # Try to remove worktree - if it fails, warn but continue
    WORKTREE_REMOVED=true
    echo "Removing worktree..."
    if ! bd worktree remove "$WORKTREE_PATH" 2>/dev/null; then
        WORKTREE_REMOVED=false
        echo ""
        echo "⚠️  Warning: Could not remove worktree at $WORKTREE_PATH"
        echo "   The worktree may have uncommitted changes or other issues."
        echo ""

        # Show what's in the worktree that might be blocking removal
        if [ -d "$WORKTREE_PATH" ]; then
            echo "   Worktree status:"

            # Check for uncommitted changes
            UNCOMMITTED=$(cd "$WORKTREE_PATH" && git status --porcelain 2>/dev/null)
            if [ -n "$UNCOMMITTED" ]; then
                echo "   - Uncommitted changes:"
                echo "$UNCOMMITTED" | sed 's/^/       /'
            fi

            # Check for unpushed commits (compare to main)
            UNPUSHED=$(cd "$WORKTREE_PATH" && git log main..HEAD --oneline 2>/dev/null)
            if [ -n "$UNPUSHED" ]; then
                echo "   - Commits not in main (should be merged now):"
                echo "$UNPUSHED" | sed 's/^/       /'
            fi

            echo ""
            echo "   To manually clean up, run:"
            echo "     rm -rf $WORKTREE_PATH"
            echo "     git worktree prune"
        fi
        echo ""
        echo "   Continuing with remaining cleanup..."
        echo ""
    fi

    echo "Deleting branch..."
    git branch -d "$BRANCH_NAME"

    # Capture issue details before closing
    ISSUE_JSON=$(bd show "$ISSUE_ID" --json 2>/dev/null | jq '.[0]' 2>/dev/null || echo '{}')
    ISSUE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.title // "Unknown"')

    # Capture epic state before closing (for epics that might become eligible)
    EPICS_BEFORE=$(bd epic status --json 2>/dev/null || echo '[]')

    echo "Closing issue..."
    bd close "$ISSUE_ID"

    # Check for epics that became eligible for closure
    EPICS_AFTER=$(bd epic status --json 2>/dev/null || echo '[]')

    # Find epics that are now eligible but weren't before
    NEWLY_ELIGIBLE_EPICS=$(jq -n \
        --argjson before "$EPICS_BEFORE" \
        --argjson after "$EPICS_AFTER" \
        '[($after[] | select(.eligible_for_close == true) | .epic.id)] -
         [($before[] | select(.eligible_for_close == true) | .epic.id)] | .[]' 2>/dev/null || echo '')

    # Auto-close newly eligible epics
    CLOSED_EPICS=""
    if [ -n "$NEWLY_ELIGIBLE_EPICS" ]; then
        for epic_id in $NEWLY_ELIGIBLE_EPICS; do
            epic_title=$(bd show "$epic_id" --json 2>/dev/null | jq -r '.[0].title // "Unknown"' 2>/dev/null)
            echo "  → Auto-closing completed epic: $epic_id \"$epic_title\""
            bd close "$epic_id" 2>/dev/null || true
            if [ -n "$CLOSED_EPICS" ]; then
                CLOSED_EPICS="$CLOSED_EPICS,$epic_id:$epic_title"
            else
                CLOSED_EPICS="$epic_id:$epic_title"
            fi
        done
    fi

    echo ""
    if [ "$WORKTREE_REMOVED" = true ]; then
        echo "Done! Branch merged and worktree cleaned up."
    else
        echo "Done! Branch merged, issue closed, but worktree needs manual cleanup."
        echo "See warning above for details."
    fi

    # === Task Depletion Flow ===
    if [ "$NO_CONTINUE" = true ]; then
        exit 0
    fi

    echo ""
    echo "=== Completion Summary ==="
    echo "✓ Task: $ISSUE_ID \"$ISSUE_TITLE\""

    # Show any auto-closed epics
    if [ -n "$CLOSED_EPICS" ]; then
        echo ""
        echo "Epics completed:"
        IFS=',' read -ra EPIC_ARRAY <<< "$CLOSED_EPICS"
        for epic_entry in "${EPIC_ARRAY[@]}"; do
            epic_id="${epic_entry%%:*}"
            epic_title="${epic_entry#*:}"
            echo "  ✓ $epic_id \"$epic_title\" (all children done)"
        done
    fi

    echo ""

    # Check for remaining ready tasks
    READY_JSON=$(bd ready --json 2>/dev/null || echo '[]')
    READY_TASKS=$(echo "$READY_JSON" | jq '[.[] | select(.issue_type == "task")]')
    READY_COUNT=$(echo "$READY_TASKS" | jq 'length')

    if [ "$READY_COUNT" -eq 0 ]; then
        # No more ready tasks - offer to create new ones
        echo "📭 No more ready tasks in the queue!"
        echo ""
        echo "Would you like to create new tasks? (y/n)"
        read -r CREATE_TASKS

        if [ "$CREATE_TASKS" = "y" ] || [ "$CREATE_TASKS" = "Y" ]; then
            echo ""
            echo "Opening interactive task creation..."
            bd create-form

            # Check if tasks were created
            NEW_READY_JSON=$(bd ready --json 2>/dev/null || echo '[]')
            NEW_READY_COUNT=$(echo "$NEW_READY_JSON" | jq '[.[] | select(.issue_type == "task")] | length')

            if [ "$NEW_READY_COUNT" -gt 0 ]; then
                echo ""
                echo "New tasks available! Spawn agent on next task? (y/n)"
                read -r SPAWN_AGENT

                if [ "$SPAWN_AGENT" = "y" ] || [ "$SPAWN_AGENT" = "Y" ]; then
                    # Build completion context for the new session
                    COMPLETION_CONTEXT="Previous session completed:
- Task: $ISSUE_ID \"$ISSUE_TITLE\""

                    if [ -n "$CLOSED_EPICS" ]; then
                        COMPLETION_CONTEXT="$COMPLETION_CONTEXT
Epics completed:"
                        for epic_entry in "${EPIC_ARRAY[@]}"; do
                            epic_id="${epic_entry%%:*}"
                            epic_title="${epic_entry#*:}"
                            COMPLETION_CONTEXT="$COMPLETION_CONTEXT
- $epic_id \"$epic_title\" (all children done)"
                        done
                    fi

                    # Export context for start-agent.sh to pick up
                    export TAMBOUR_COMPLETION_CONTEXT="$COMPLETION_CONTEXT"

                    echo ""
                    exec "$REPO_ROOT/scripts/start-agent.sh"
                fi
            else
                echo "No new tasks created."
            fi
        fi
    else
        # There are more ready tasks
        NEXT_TASK=$(echo "$READY_TASKS" | jq -r '.[0].id')
        NEXT_TITLE=$(echo "$READY_TASKS" | jq -r '.[0].title')

        echo "📋 $READY_COUNT ready task(s) remaining"
        echo "   Next: $NEXT_TASK \"$NEXT_TITLE\""
        echo ""
        echo "Continue to next task? (y/n)"
        read -r CONTINUE

        if [ "$CONTINUE" = "y" ] || [ "$CONTINUE" = "Y" ]; then
            # Build completion context for the new session
            COMPLETION_CONTEXT="Previous session completed:
- Task: $ISSUE_ID \"$ISSUE_TITLE\""

            if [ -n "$CLOSED_EPICS" ]; then
                COMPLETION_CONTEXT="$COMPLETION_CONTEXT
Epics completed:"
                for epic_entry in "${EPIC_ARRAY[@]}"; do
                    epic_id="${epic_entry%%:*}"
                    epic_title="${epic_entry#*:}"
                    COMPLETION_CONTEXT="$COMPLETION_CONTEXT
- $epic_id \"$epic_title\" (all children done)"
                done
            fi

            # Export context for start-agent.sh to pick up
            export TAMBOUR_COMPLETION_CONTEXT="$COMPLETION_CONTEXT"

            echo ""
            exec "$REPO_ROOT/scripts/start-agent.sh"
        fi
    fi
else
    echo "Worktree preserved at: $WORKTREE_PATH"
    echo ""
    echo "To merge and cleanup later, run:"
    echo "  $0 $ISSUE_ID --merge"
    echo ""
    echo "Or manually:"
    echo "  git checkout main"
    echo "  git merge $BRANCH_NAME"
    echo "  bd worktree remove $WORKTREE_PATH"
    echo "  git branch -d $BRANCH_NAME"
    echo "  bd close $ISSUE_ID"
fi
