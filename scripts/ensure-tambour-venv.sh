#!/usr/bin/env bash
# Auto-setup Python venv with tambour installed for tests and tooling.
#
# Tambour source discovery priority:
#   1. $TAMBOUR_DIR env var
#   2. ./tambour/ (in-repo subdirectory)
#   3. Gas Town rig auto-discovery (walks up to find town root)
#
# Creates .venv at repo root and records source path in .tambour-source.
set -e

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENV_DIR="$REPO_ROOT/.venv"
SOURCE_FILE="$REPO_ROOT/.tambour-source"

find_tambour() {
    # 1. Explicit env var
    if [ -n "$TAMBOUR_DIR" ] && [ -f "$TAMBOUR_DIR/pyproject.toml" ]; then
        echo "$TAMBOUR_DIR"
        return 0
    fi

    # 2. In-repo subdirectory (legacy bundled layout)
    if [ -f "$REPO_ROOT/tambour/pyproject.toml" ]; then
        echo "$REPO_ROOT/tambour"
        return 0
    fi

    # 3. Walk up from repo root to find Gas Town town root,
    #    then look for tambour rig at tambour/mayor/rig/
    local dir="$REPO_ROOT"
    while [ "$dir" != "/" ]; do
        dir="$(dirname "$dir")"
        if [ -d "$dir/tambour/mayor/rig/src/tambour" ]; then
            echo "$dir/tambour/mayor/rig"
            return 0
        fi
    done

    return 1
}

TAMBOUR_SRC=$(find_tambour) || {
    echo "Error: Cannot find tambour source directory." >&2
    echo "Set TAMBOUR_DIR to the path containing tambour's pyproject.toml" >&2
    exit 1
}

# Create venv if it doesn't exist
if [ ! -f "$VENV_DIR/bin/python" ]; then
    echo "Creating Python venv at .venv..."
    python3 -m venv "$VENV_DIR"
fi

# Install tambour with dev deps (pytest)
echo "Installing tambour from $TAMBOUR_SRC..."
"$VENV_DIR/bin/pip" install -q -e "$TAMBOUR_SRC[dev]"

# Record source location for other scripts/recipes
echo "$TAMBOUR_SRC" > "$SOURCE_FILE"

echo "Tambour venv ready (source: $TAMBOUR_SRC)"
