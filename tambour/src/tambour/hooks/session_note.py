"""Claude Code SessionStart hook for auto-setting session notes.

Automatically sets the session note from the beads issue title when
starting a session in a worktree.

Usage (in .claude/settings.local.json):
    {
      "hooks": {
        "SessionStart": [{
          "hooks": [{
            "type": "command",
            "command": "/path/to/scripts/session-note-hook.sh",
            "timeout": 5
          }]
        }]
      }
    }
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path


def parse_stdin() -> dict | None:
    """Read and parse JSON from stdin."""
    try:
        data = sys.stdin.read()
        if not data.strip():
            return None
        return json.loads(data)
    except (json.JSONDecodeError, Exception):
        return None


def infer_issue_id(cwd: str) -> str | None:
    """Infer issue ID from the current working directory.

    Checks for worktree patterns or git branch name matching issue patterns.
    """
    path = Path(cwd)

    # Check if this looks like a worktree path
    if "-worktrees" in str(path):
        return path.name

    # Check if directory name matches issue pattern (e.g., bobbin-abc)
    name = path.name
    if re.match(r"^[a-z]+-[a-z0-9]+(\.[a-z0-9]+)*$", name, re.IGNORECASE):
        return name

    # Try to get the git branch name
    try:
        result = subprocess.run(
            ["git", "branch", "--show-current"],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=2,
        )
        if result.returncode == 0:
            branch = result.stdout.strip()
            # Check if branch matches issue pattern
            if re.match(r"^[a-z]+-[a-z0-9]+(\.[a-z0-9]+)*$", branch, re.IGNORECASE):
                return branch
    except Exception:
        pass

    return None


def get_issue_title(issue_id: str) -> str | None:
    """Get issue title from beads."""
    try:
        result = subprocess.run(
            ["bd", "show", issue_id, "--json"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            if data and isinstance(data, list) and len(data) > 0:
                return data[0].get("title")
    except Exception:
        pass
    return None


def truncate_to_words(text: str, max_words: int = 3) -> str:
    """Truncate text to max_words words."""
    words = text.split()
    if len(words) <= max_words:
        return text
    return " ".join(words[:max_words])


def write_session_note(session_id: str, note: str) -> bool:
    """Write the session note file."""
    try:
        notes_dir = Path.home() / ".claude" / "session_notes"
        notes_dir.mkdir(parents=True, exist_ok=True)
        note_file = notes_dir / f"{session_id}.txt"
        note_file.write_text(note)
        return True
    except Exception:
        return False


def main() -> int:
    """Main entry point.

    Reads session.start hook data from stdin, detects if we're in an issue
    worktree, and sets the session note from the issue title.
    """
    hook_data = parse_stdin()
    if hook_data is None:
        return 0

    session_id = hook_data.get("session_id", "")
    cwd = hook_data.get("cwd", os.getcwd())

    if not session_id:
        return 0

    # Try to infer issue ID from context
    issue_id = infer_issue_id(cwd)
    if not issue_id:
        return 0

    # Get the issue title
    title = get_issue_title(issue_id)
    if not title:
        # Fallback: use issue ID as note
        title = issue_id

    # Truncate to ~3 words for statusline
    note = truncate_to_words(title, max_words=3)

    # Write the session note
    write_session_note(session_id, note)

    return 0


if __name__ == "__main__":
    sys.exit(main())
