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

    echo "Removing worktree..."
    bd worktree remove "$WORKTREE_PATH"

    echo "Deleting branch..."
    git branch -d "$BRANCH_NAME"

    echo "Closing issue..."
    bd close "$ISSUE_ID"

    echo ""
    echo "Done! Branch merged and worktree cleaned up."
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
