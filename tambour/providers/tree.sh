#!/bin/bash
# Context provider: Directory tree
#
# Generates an ASCII representation of the project structure.
# Excludes common directories that clutter output.
#
# Environment variables (from tambour):
#   TAMBOUR_WORKTREE - Working directory (defaults to pwd)
#
# Configuration via environment:
#   TREE_DEPTH   - Maximum depth (default: 4)
#   TREE_EXCLUDE - Additional comma-separated patterns to exclude

set -e

DIR="${TAMBOUR_WORKTREE:-$(pwd)}"
DEPTH="${TREE_DEPTH:-4}"

# Patterns to exclude (common noise directories)
EXCLUDES=".git|node_modules|target|dist|build|__pycache__|.venv|venv|*.pyc|.DS_Store|.idea|.vscode|*.egg-info|.mypy_cache|.pytest_cache|.coverage|.tox|.nox"

# Add user exclusions if provided
if [ -n "$TREE_EXCLUDE" ]; then
    EXCLUDES="${EXCLUDES}|${TREE_EXCLUDE//,/|}"
fi

echo "# Project Structure"
echo ""

cd "$DIR"

if command -v tree &> /dev/null; then
    # Use tree command (preferred)
    tree -L "$DEPTH" --noreport -I "$EXCLUDES" .
else
    # Fallback: simple find-based listing
    echo "."
    find . -maxdepth "$DEPTH" -type f -o -type d 2>/dev/null | \
        grep -Ev "($EXCLUDES)" | \
        sort | \
        sed 's|^./||' | \
        while read -r path; do
            depth=$(($(echo "$path" | tr -cd '/' | wc -c)))
            indent=$(printf '%*s' $((depth * 2)) '')
            name=$(basename "$path")
            if [ -d "$path" ]; then
                echo "${indent}${name}/"
            else
                echo "${indent}${name}"
            fi
        done
fi
