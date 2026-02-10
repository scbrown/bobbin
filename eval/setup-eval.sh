#!/usr/bin/env bash
# Setup script for bobbin eval environment.
# Run once before running evaluations.
set -euo pipefail

echo "=== Bobbin Eval Setup ==="

# Check/install uv (Python package manager).
if ! command -v uv &>/dev/null; then
    echo "Installing uv..."
    pip3 install --break-system-packages uv
else
    echo "uv: $(uv --version)"
fi

# Check/install tokei (LOC counter).
if ! command -v tokei &>/dev/null; then
    echo "Installing tokei..."
    cargo install tokei
else
    echo "tokei: $(tokei --version)"
fi

# Check/symlink bobbin.
if ! command -v bobbin &>/dev/null; then
    EVAL_DIR="$(cd "$(dirname "$0")" && pwd)"
    BOBBIN_BIN="$EVAL_DIR/../target/debug/bobbin"
    if [ -f "$BOBBIN_BIN" ]; then
        echo "Symlinking bobbin from $BOBBIN_BIN"
        ln -sf "$BOBBIN_BIN" "$HOME/.local/bin/bobbin"
    else
        echo "ERROR: bobbin binary not found. Build with: cargo build"
        exit 1
    fi
else
    echo "bobbin: $(bobbin --version)"
fi

# Check claude CLI.
if ! command -v claude &>/dev/null; then
    echo "ERROR: claude CLI not found. Install Claude Code first."
    exit 1
else
    echo "claude: found at $(command -v claude)"
fi

# Check matplotlib (for charts).
python3 -c "import matplotlib" 2>/dev/null || {
    echo "Installing matplotlib..."
    pip3 install --break-system-packages matplotlib
}

echo ""
echo "=== All prerequisites satisfied ==="
echo "Run evaluations with:"
echo "  cd eval/"
echo "  python3 -m runner.cli run-all --attempts 3 --approaches both"
