#!/bin/bash
# Cleanup after agent completes work in a worktree
#
# Usage: ./scripts/finish-agent.sh <issue-id> [--merge]
#   --merge: merge the branch back to main and remove worktree

set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKTREE_BASE="${REPO_ROOT}/../bobbin-worktrees"

if [ -z "$1" ]; then
    echo "Usage: $0 <issue-id> [--merge]"
    echo ""
    echo "Active worktrees:"
    bd worktree list
    exit 1
fi

ISSUE_ID="$1"
WORKTREE_PATH="${WORKTREE_BASE}/${ISSUE_ID}"
BRANCH_NAME="$ISSUE_ID"
DO_MERGE=false

if [ "$2" = "--merge" ]; then
    DO_MERGE=true
fi

cd "$REPO_ROOT"

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

    echo "Closing issue..."
    bd close "$ISSUE_ID"

    echo ""
    if [ "$WORKTREE_REMOVED" = true ]; then
        echo "Done! Branch merged and worktree cleaned up."
    else
        echo "Done! Branch merged, issue closed, but worktree needs manual cleanup."
        echo "See warning above for details."
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
