#!/bin/bash
# Claude Code session.start hook for auto-setting session notes
# Sets the session note from beads issue title when in a worktree

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# Set PYTHONPATH and run the hook
PYTHONPATH="$REPO_ROOT/tambour/src" exec python3 -m tambour.hooks.session_note
