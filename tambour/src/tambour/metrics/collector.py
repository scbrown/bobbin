"""Metrics collector plugin for tambour.

This module implements the metrics-collector plugin that subscribes to tool.*
events and stores them to JSONL for later analysis.

Usage:
    python -m tambour.metrics collect

Plugin Configuration:
    [plugins.metrics-collector]
    on = ["tool.used", "tool.failed"]
    run = "python -m tambour.metrics collect"
    blocking = false
    timeout = 5
"""

from __future__ import annotations

import json
import os
import sys
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from tambour.metrics.extractors import extract_tool_fields


@dataclass
class MetricEvent:
    """A metric event to be stored in JSONL.

    Attributes:
        timestamp: When the event occurred (ISO format).
        session_id: Claude Code session identifier.
        issue_id: Associated beads issue ID (if in a worktree).
        worktree: Path to the worktree (if applicable).
        tool: Name of the tool (Read, Write, Edit, etc.).
        input: Tool-specific input fields.
        output: Tool response/result.
        error: Error message if the tool failed.
    """

    timestamp: str
    session_id: str
    tool: str
    input: dict[str, Any]
    output: dict[str, Any] | None = None
    issue_id: str | None = None
    worktree: str | None = None
    error: str | None = None

    def to_json(self) -> str:
        """Convert to JSON string for JSONL storage."""
        data = asdict(self)
        # Remove None values for cleaner output
        data = {k: v for k, v in data.items() if v is not None}
        return json.dumps(data, separators=(",", ":"))


class MetricsCollector:
    """Collects and stores tool use metrics.

    Receives event data from the tambour event dispatcher and appends
    metric events to JSONL storage.
    """

    DEFAULT_METRICS_PATH = ".tambour/metrics.jsonl"

    def __init__(self, storage_path: Path | None = None):
        """Initialize the collector.

        Args:
            storage_path: Path to the JSONL file. Defaults to .tambour/metrics.jsonl
                         in the current working directory.
        """
        if storage_path is None:
            storage_path = Path.cwd() / self.DEFAULT_METRICS_PATH
        self.storage_path = Path(storage_path)

    def collect_from_env(self) -> MetricEvent | None:
        """Collect a metric event from environment variables.

        Reads event data from TAMBOUR_* environment variables set by
        the event dispatcher.

        Returns:
            MetricEvent if collection succeeded, None if required data missing.
        """
        # Read required fields
        event_type = os.environ.get("TAMBOUR_EVENT")
        tool_name = os.environ.get("TAMBOUR_TOOL_NAME")
        session_id = os.environ.get("TAMBOUR_SESSION_ID", "unknown")
        timestamp = os.environ.get("TAMBOUR_TIMESTAMP")

        if not event_type or not tool_name:
            return None

        # Use current time if timestamp not provided
        if not timestamp:
            timestamp = datetime.now(timezone.utc).isoformat()

        # Read optional fields
        issue_id = os.environ.get("TAMBOUR_ISSUE_ID")
        worktree = os.environ.get("TAMBOUR_WORKTREE")
        error = os.environ.get("TAMBOUR_ERROR")

        # Get any additional tool-specific data from environment
        # The bridge script may have stored tool_input as JSON in TAMBOUR_TOOL_INPUT
        tool_input_str = os.environ.get("TAMBOUR_TOOL_INPUT", "{}")
        try:
            tool_input = json.loads(tool_input_str)
        except json.JSONDecodeError:
            tool_input = {}

        # Fallback: check for specific fields passed directly as env vars
        # (e.g., TAMBOUR_FILE_PATH, TAMBOUR_COMMAND)
        if not tool_input:
            tool_input = self._collect_tool_input_from_env(tool_name)

        # Extract relevant fields based on tool type
        extracted_input = extract_tool_fields(tool_name, tool_input)

        # Get tool response/output if available
        tool_output_str = os.environ.get("TAMBOUR_TOOL_OUTPUT", "{}")
        try:
            tool_output = json.loads(tool_output_str)
        except json.JSONDecodeError:
            tool_output = {}

        # If no structured output, check for success indicator
        if not tool_output:
            success = os.environ.get("TAMBOUR_SUCCESS")
            if success is not None:
                tool_output = {"success": success.lower() == "true"}

        return MetricEvent(
            timestamp=timestamp,
            session_id=session_id,
            tool=tool_name,
            input=extracted_input,
            output=tool_output if tool_output else None,
            issue_id=issue_id,
            worktree=worktree,
            error=error,
        )

    def _collect_tool_input_from_env(self, tool_name: str) -> dict[str, Any]:
        """Collect tool input from individual environment variables.

        Args:
            tool_name: The name of the tool.

        Returns:
            Dict with tool input fields.
        """
        result: dict[str, Any] = {}

        # Common fields
        file_path = os.environ.get("TAMBOUR_FILE_PATH")
        if file_path:
            result["file_path"] = file_path

        # Bash-specific
        command = os.environ.get("TAMBOUR_COMMAND")
        if command:
            result["command"] = command

        description = os.environ.get("TAMBOUR_DESCRIPTION")
        if description:
            result["description"] = description

        # Search tools
        pattern = os.environ.get("TAMBOUR_PATTERN")
        if pattern:
            result["pattern"] = pattern

        path = os.environ.get("TAMBOUR_PATH")
        if path:
            result["path"] = path

        return result

    def collect_from_stdin(self) -> MetricEvent | None:
        """Collect a metric event from JSON on stdin.

        Alternative collection method for direct JSON input.

        Returns:
            MetricEvent if collection succeeded, None if invalid input.
        """
        try:
            data = sys.stdin.read()
            if not data.strip():
                return None

            event_data = json.loads(data)
            return self._parse_event_data(event_data)
        except json.JSONDecodeError:
            return None
        except Exception:
            return None

    def _parse_event_data(self, event_data: dict[str, Any]) -> MetricEvent | None:
        """Parse event data dict into a MetricEvent.

        Args:
            event_data: Dict with event data fields.

        Returns:
            MetricEvent if parsing succeeded, None otherwise.
        """
        # Handle nested "data" field (from event dispatcher format)
        if "data" in event_data and isinstance(event_data["data"], dict):
            inner_data = event_data["data"]
            tool_name = inner_data.get("tool_name")
            tool_input = inner_data.get("tool_input", {})
            tool_response = inner_data.get("tool_response", {})
            session_id = inner_data.get("session_id", "unknown")
            issue_id = inner_data.get("issue_id")
            worktree = inner_data.get("worktree")
        else:
            # Flat format
            tool_name = event_data.get("tool_name") or event_data.get("tool")
            tool_input = event_data.get("tool_input", {})
            tool_response = event_data.get("tool_response", {})
            session_id = event_data.get("session_id", "unknown")
            issue_id = event_data.get("issue_id")
            worktree = event_data.get("worktree")

        if not tool_name:
            return None

        timestamp = event_data.get("timestamp")
        if not timestamp:
            timestamp = datetime.now(timezone.utc).isoformat()

        # Extract relevant fields based on tool type
        extracted_input = extract_tool_fields(tool_name, tool_input)

        # Check for error
        error = None
        event_type = event_data.get("event_type", "")
        if event_type == "tool.failed" or "failed" in event_type:
            error = tool_response.get("error") or event_data.get("error")

        return MetricEvent(
            timestamp=timestamp,
            session_id=session_id,
            tool=tool_name,
            input=extracted_input,
            output=tool_response if tool_response else None,
            issue_id=issue_id,
            worktree=worktree,
            error=error,
        )

    def store(self, event: MetricEvent) -> bool:
        """Store a metric event to JSONL.

        Args:
            event: The metric event to store.

        Returns:
            True if storage succeeded, False otherwise.
        """
        try:
            # Create directory if it doesn't exist
            self.storage_path.parent.mkdir(parents=True, exist_ok=True)

            # Append to JSONL file
            with open(self.storage_path, "a") as f:
                f.write(event.to_json() + "\n")

            return True
        except Exception as e:
            # Log error but don't crash
            print(f"Error storing metric: {e}", file=sys.stderr)
            return False

    def collect_and_store(self) -> bool:
        """Collect from environment and store.

        Main entry point for plugin execution. Attempts to collect
        from environment variables first, then falls back to stdin.

        Returns:
            True if an event was collected and stored, False otherwise.
        """
        # Try environment variables first (set by event dispatcher)
        event = self.collect_from_env()

        # If no env vars, try stdin (for direct invocation)
        if event is None:
            event = self.collect_from_stdin()

        if event is None:
            return False

        return self.store(event)


def main() -> int:
    """CLI entry point for the metrics collector.

    Called as: python -m tambour.metrics collect

    Returns:
        0 on success, 1 on failure.
    """
    collector = MetricsCollector()
    success = collector.collect_and_store()
    return 0 if success else 0  # Always return 0 to not block event dispatch


if __name__ == "__main__":
    sys.exit(main())
