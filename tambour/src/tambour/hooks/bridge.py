"""Claude Code PostToolUse hook bridge.

Receives Claude Code hook events via stdin JSON and emits tambour events.

Usage:
    python -m tambour.hooks.bridge

Architecture:
    Claude Code PostToolUse hook
            ↓ JSON via stdin
    Bridge script (this module)
            ↓
    python -m tambour events emit tool.used --data '...'
            ↓
    Tambour event dispatcher
            ↓
    Subscribed plugins (metrics-collector, etc.)
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


def parse_stdin() -> dict[str, Any] | None:
    """Read and parse JSON from stdin.

    Returns:
        Parsed JSON dict, or None if input is empty/invalid.
    """
    try:
        data = sys.stdin.read()
        if not data.strip():
            return None
        return json.loads(data)
    except json.JSONDecodeError:
        return None
    except Exception:
        return None


def detect_failure(tool_response: dict[str, Any]) -> tuple[bool, str | None]:
    """Detect if a tool response indicates failure.

    Args:
        tool_response: The tool_response dict from the hook.

    Returns:
        Tuple of (is_failed, error_message).
    """
    # Check for explicit failure indicators
    if isinstance(tool_response, dict):
        # Check for error field
        if "error" in tool_response:
            return True, str(tool_response["error"])

        # Check for success: false
        success = tool_response.get("success")
        if success is False or str(success).lower() == "false":
            error = tool_response.get("message", "Tool reported failure")
            return True, str(error)

        # Check for is_error field (Claude Code convention)
        if tool_response.get("is_error"):
            error = tool_response.get("content", "Tool error")
            return True, str(error)

    return False, None


def infer_issue_id(cwd: str) -> str | None:
    """Infer issue ID from the current working directory.

    Assumes worktrees are named after the issue ID (e.g., bobbin-xyz).

    Args:
        cwd: The current working directory path.

    Returns:
        Issue ID if detected, None otherwise.
    """
    path = Path(cwd)

    # Check if this looks like a worktree path
    # Pattern: *-worktrees/issue-id or just the directory name matching issue pattern
    if "-worktrees" in str(path):
        # Extract the last component (issue ID)
        return path.name

    # Check if the directory name matches common issue ID patterns
    name = path.name
    # Matches patterns like: bobbin-abc, proj-123, issue-xyz
    if re.match(r"^[a-z]+-[a-z0-9]+(\.[a-z0-9]+)*$", name, re.IGNORECASE):
        return name

    return None


def emit_event(
    event_type: str,
    tool_name: str,
    session_id: str,
    issue_id: str | None,
    worktree: str | None,
    extra_data: dict[str, Any] | None = None,
) -> int:
    """Emit a tambour event via the CLI.

    Args:
        event_type: The event type (tool.used or tool.failed).
        tool_name: Name of the tool that was used.
        session_id: Claude Code session identifier.
        issue_id: Optional issue ID.
        worktree: Optional worktree path.
        extra_data: Additional data to include in the event.

    Returns:
        Exit code from the tambour CLI (0 for success).
    """
    data = {
        "tool_name": tool_name,
        "session_id": session_id,
    }
    if extra_data:
        data.update(extra_data)

    cmd = [
        sys.executable,
        "-m",
        "tambour",
        "events",
        "emit",
        event_type,
        "--data",
        json.dumps(data),
    ]

    if issue_id:
        cmd.extend(["--issue", issue_id])

    if worktree:
        cmd.extend(["--worktree", worktree])

    try:
        # Run with minimal overhead
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=3,  # Short timeout to keep overall hook fast
        )
        return result.returncode
    except subprocess.TimeoutExpired:
        return 1
    except Exception:
        return 1


def main() -> int:
    """Main entry point for the bridge.

    Reads JSON from stdin, determines if the tool succeeded or failed,
    and emits the appropriate tambour event.

    Returns:
        0 on success, 1 on failure.
    """
    # Parse input
    hook_data = parse_stdin()
    if hook_data is None:
        # No input or invalid JSON - fail silently
        return 0

    # Extract required fields
    tool_name = hook_data.get("tool_name", "")
    session_id = hook_data.get("session_id", "")
    tool_input = hook_data.get("tool_input", {})
    tool_response = hook_data.get("tool_response", {})
    cwd = hook_data.get("cwd", "")

    if not tool_name:
        # Missing critical field - fail silently
        return 0

    # Use session ID or generate a placeholder
    if not session_id:
        session_id = "unknown"

    # Detect if this was a failure
    is_failed, error_msg = detect_failure(tool_response)

    # Infer issue ID from cwd
    issue_id = infer_issue_id(cwd) if cwd else None

    # Build extra data
    extra_data: dict[str, Any] = {}

    # For Read tool, include file path
    if tool_name == "Read" and isinstance(tool_input, dict):
        file_path = tool_input.get("file_path")
        if file_path:
            extra_data["file_path"] = file_path

    # For Write/Edit, include file path
    if tool_name in ("Write", "Edit") and isinstance(tool_input, dict):
        file_path = tool_input.get("file_path")
        if file_path:
            extra_data["file_path"] = file_path

    # For Bash, include a truncated command
    if tool_name == "Bash" and isinstance(tool_input, dict):
        command = tool_input.get("command", "")
        if command:
            # Truncate long commands
            extra_data["command"] = command[:200]

    # Include error message if failed
    if is_failed and error_msg:
        extra_data["error"] = error_msg

    # Emit the appropriate event
    event_type = "tool.failed" if is_failed else "tool.used"
    return emit_event(
        event_type=event_type,
        tool_name=tool_name,
        session_id=session_id,
        issue_id=issue_id,
        worktree=cwd if cwd else None,
        extra_data=extra_data if extra_data else None,
    )


if __name__ == "__main__":
    sys.exit(main())
