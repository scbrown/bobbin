#!/usr/bin/env bash
# Setup tambour hooks for metrics collection
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TAMBOUR_DIR="$(dirname "$SCRIPT_DIR")"
REPO_DIR="$(dirname "$TAMBOUR_DIR")"
CLAUDE_DIR="$HOME/.claude"

echo "=== Setting up Tambour ==="

# 1. Create/update venv
echo "→ Setting up Python venv..."
cd "$TAMBOUR_DIR"
if [ ! -d ".venv" ]; then
    python3 -m venv .venv
fi
source .venv/bin/activate
pip install -q -e .

# 2. Create wrapper script for hook
echo "→ Creating hook wrapper..."
mkdir -p "$CLAUDE_DIR"
cat > "$CLAUDE_DIR/tambour-hook.sh" << EOF
#!/usr/bin/env bash
# Tambour PostToolUse hook wrapper
# Activates venv and runs the bridge
TAMBOUR_VENV="$TAMBOUR_DIR/.venv"
if [ -f "\$TAMBOUR_VENV/bin/python" ]; then
    "\$TAMBOUR_VENV/bin/python" -m tambour.hooks.bridge
fi
EOF
chmod +x "$CLAUDE_DIR/tambour-hook.sh"

# 3. Update .tambour/config.toml with correct venv path
echo "→ Updating tambour config..."
TAMBOUR_CONFIG="$REPO_DIR/.tambour/config.toml"
VENV_PYTHON="$TAMBOUR_DIR/.venv/bin/python"

if [ -f "$TAMBOUR_CONFIG" ]; then
    # Replace any 'python -m tambour' with the venv python path
    # This handles both 'python' and 'python3' variants
    sed -i.bak -E "s|run = \"python3? -m tambour|run = \"$VENV_PYTHON -m tambour|g" "$TAMBOUR_CONFIG"
    rm -f "$TAMBOUR_CONFIG.bak"
    echo "   Updated $TAMBOUR_CONFIG"
fi

# 4. Update Claude settings.json
echo "→ Configuring Claude Code hooks..."
SETTINGS="$CLAUDE_DIR/settings.json"

python3 << EOF
import json
from pathlib import Path

settings_path = Path("$SETTINGS")

if settings_path.exists():
    settings = json.loads(settings_path.read_text())
else:
    settings = {}

# Add hooks section if not present
if "hooks" not in settings:
    settings["hooks"] = {}

# Add PostToolUse hook
settings["hooks"]["PostToolUse"] = [
    {
        "matcher": ".*",
        "hooks": [
            {
                "type": "command",
                "command": "$CLAUDE_DIR/tambour-hook.sh"
            }
        ]
    }
]

settings_path.write_text(json.dumps(settings, indent=2))
print("   Updated", str(settings_path))
EOF

echo ""
echo "✓ Tambour setup complete!"
echo ""
echo "Metrics will now be collected for all tool usage."
echo "View metrics with: just tambour metrics"
