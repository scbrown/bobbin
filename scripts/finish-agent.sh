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
RUNNING_FROM_WORKTREE=false
if [ -f "$GIT_TOPLEVEL/.git" ]; then
    # We're in a worktree - need to find the main repo
    # The .git file contains: "gitdir: /path/to/main/.git/worktrees/branch-name"
    MAIN_GIT_DIR="$(cat "$GIT_TOPLEVEL/.git" | sed 's/^gitdir: //' | sed 's|/worktrees/.*||')"
    REPO_ROOT="$(dirname "$MAIN_GIT_DIR")"
    RUNNING_FROM_WORKTREE=true
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

# Event emission helper
emit_event() {
    local event_type="$1"
    local issue_id="$2"
    local worktree="$3"
    shift 3
    local extra_args=($@)

    if command -v python3 &> /dev/null; then
        PYTHONPATH="$REPO_ROOT/tambour/src" python3 -m tambour events emit "$event_type" \
            ${issue_id:+--issue "$issue_id"} \
            ${worktree:+--worktree "$worktree"} \
            --main-repo "$REPO_ROOT" \
            --beads-db "$REPO_ROOT/.beads" \
            "${extra_args[@]}" \
            2>/dev/null || true
    fi
}

# Check if we're running from inside the target worktree
RUNNING_FROM_TARGET_WORKTREE=false
if [ "$RUNNING_FROM_WORKTREE" = true ]; then
    # Get the absolute path of both directories and compare
    TARGET_ABS="$(cd "$WORKTREE_PATH" 2>/dev/null && pwd)" || true
    CURRENT_ABS="$GIT_TOPLEVEL"
    if [ "$TARGET_ABS" = "$CURRENT_ABS" ]; then
        RUNNING_FROM_TARGET_WORKTREE=true
    fi
fi

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

    # Check if branch exists before merging
    if git show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
        # Merge the branch
        git merge "$BRANCH_NAME" --no-edit
    else
        echo "Branch $BRANCH_NAME does not exist. Assuming changes are already merged or discarded."
    fi

    # Emit branch.merged event
    emit_event "branch.merged" "$ISSUE_ID" "$WORKTREE_PATH"

    # Before removing the worktree, detach HEAD there so the branch isn't checked out
    # This is necessary because git won't let us delete a branch that's checked out anywhere
    echo "Detaching HEAD in worktree..."
    if ! git -C "$WORKTREE_PATH" checkout --detach 2>/dev/null; then
        echo "Warning: Could not detach HEAD in worktree (may already be detached)"
    fi

    # Try to remove worktree - if it fails, warn but continue
    WORKTREE_REMOVED=true
    echo "Removing worktree..."
    if ! bd worktree remove "$WORKTREE_PATH" 2>/dev/null; then
        WORKTREE_REMOVED=false

        if [ "$RUNNING_FROM_TARGET_WORKTREE" = true ]; then
            # Expected failure - we're inside the worktree we're trying to remove
            echo "   (Worktree in use - will need manual cleanup after exit)"
        else
            # Unexpected failure - show diagnostics
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
    fi

    echo "Deleting branch..."
    # Use -d normally, but if that fails (e.g., worktree still exists but detached),
    # the branch should still be deletable since we detached HEAD
    if git show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
        if ! git branch -d "$BRANCH_NAME" 2>/dev/null; then
            # Branch might still show as checked out if worktree wasn't removed
            # Since we detached HEAD, force delete should be safe (changes are merged)
            echo "Standard delete failed, trying force delete (branch is merged)..."
            git branch -D "$BRANCH_NAME"
        fi
    else
        echo "Branch $BRANCH_NAME already deleted."
    fi

    # Capture issue details before closing
    ISSUE_JSON=$(bd show "$ISSUE_ID" --json 2>/dev/null | jq '.[0]' 2>/dev/null || echo '{}')
    ISSUE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.title // "Unknown"')

    # Capture epic state before closing (for epics that might become eligible)
    EPICS_BEFORE=$(bd epic status --json 2>/dev/null || echo '[]')

    echo "Closing issue..."
    # Check if issue is already closed
    ISSUE_STATUS=$(echo "$ISSUE_JSON" | jq -r '.status // "unknown"')
    if [ "$ISSUE_STATUS" = "closed" ] || [ "$ISSUE_STATUS" = "done" ]; then
        echo "Issue $ISSUE_ID is already closed."
    else
        bd close "$ISSUE_ID" || echo "Warning: Could not close issue $ISSUE_ID"
    fi

    # Emit task.completed event
    emit_event "task.completed" "$ISSUE_ID" "$WORKTREE_PATH"

    # Check for epics that became eligible for closure
    EPICS_AFTER=$(bd epic status --json 2>/dev/null || echo '[]')

    # Find epics that are now eligible but weren't before
    NEWLY_ELIGIBLE_EPICS=$(jq -n \
        --argjson before "$EPICS_BEFORE" \
        --argjson after "$EPICS_AFTER" \
        '[($after[] | select(.eligible_for_close == true) | .epic.id)] - \
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
    elif [ "$RUNNING_FROM_TARGET_WORKTREE" = true ]; then
        echo "Done! Branch merged and issue closed."
        echo ""
        echo "To clean up this worktree after exiting, run:"
        echo "  rm -rf $WORKTREE_PATH && git -C $REPO_ROOT worktree prune"
    else
        echo "Done! Branch merged, issue closed, but worktree needs manual cleanup."
        echo "See warning above for details."
    fi

    # === Task Depletion Flow ===
    # Skip if --no-continue was passed, or if running from inside the target worktree
    # (can't spawn new agent from inside a worktree we just finished), or if not interactive
    if [ "$NO_CONTINUE" = true ] || [ "$RUNNING_FROM_TARGET_WORKTREE" = true ] || [ ! -t 0 ]; then
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
