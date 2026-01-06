"""Tests for metrics collector plugin."""

from __future__ import annotations

import json
import os
from pathlib import Path
from unittest.mock import patch

import pytest

from tambour.metrics.collector import MetricEvent, MetricsCollector
from tambour.metrics.extractors import (
    extract_bash_fields,
    extract_edit_fields,
    extract_glob_fields,
    extract_grep_fields,
    extract_read_fields,
    extract_task_fields,
    extract_tool_fields,
    extract_webfetch_fields,
    extract_websearch_fields,
    extract_write_fields,
)


class TestExtractors:
    """Tests for tool-specific field extractors."""

    def test_extract_read_fields(self):
        """Test Read tool field extraction."""
        tool_input = {
            "file_path": "/path/to/file.py",
            "offset": 100,
            "limit": 50,
        }
        result = extract_read_fields(tool_input)

        assert result["file_path"] == "/path/to/file.py"
        assert result["offset"] == 100
        assert result["limit"] == 50

    def test_extract_read_fields_minimal(self):
        """Test Read extraction with only file_path."""
        tool_input = {"file_path": "/path/to/file.py"}
        result = extract_read_fields(tool_input)

        assert result["file_path"] == "/path/to/file.py"
        assert result["offset"] is None
        assert result["limit"] is None

    def test_extract_write_fields(self):
        """Test Write tool field extraction."""
        tool_input = {
            "file_path": "/path/to/file.py",
            "content": "print('hello world')\n",
        }
        result = extract_write_fields(tool_input)

        assert result["file_path"] == "/path/to/file.py"
        assert result["content_length"] == 21

    def test_extract_write_fields_no_content(self):
        """Test Write extraction with missing content."""
        tool_input = {"file_path": "/path/to/file.py"}
        result = extract_write_fields(tool_input)

        assert result["file_path"] == "/path/to/file.py"
        assert result["content_length"] == 0  # Empty string length

    def test_extract_edit_fields(self):
        """Test Edit tool field extraction."""
        tool_input = {
            "file_path": "/path/to/file.py",
            "old_string": "foo",
            "new_string": "bar_baz",
        }
        result = extract_edit_fields(tool_input)

        assert result["file_path"] == "/path/to/file.py"
        assert result["old_string_len"] == 3
        assert result["new_string_len"] == 7

    def test_extract_glob_fields(self):
        """Test Glob tool field extraction."""
        tool_input = {
            "pattern": "**/*.py",
            "path": "/path/to/search",
        }
        result = extract_glob_fields(tool_input)

        assert result["pattern"] == "**/*.py"
        assert result["path"] == "/path/to/search"

    def test_extract_grep_fields(self):
        """Test Grep tool field extraction."""
        tool_input = {
            "pattern": "def main",
            "path": "/path/to/search",
            "output_mode": "content",
        }
        result = extract_grep_fields(tool_input)

        assert result["pattern"] == "def main"
        assert result["path"] == "/path/to/search"
        assert result["output_mode"] == "content"

    def test_extract_bash_fields(self):
        """Test Bash tool field extraction."""
        tool_input = {
            "command": "git status --short",
            "description": "Check git status",
        }
        result = extract_bash_fields(tool_input)

        assert result["command_prefix"] == "git"
        assert result["description"] == "Check git status"

    def test_extract_bash_fields_empty_command(self):
        """Test Bash extraction with empty command."""
        tool_input = {"command": "", "description": "Nothing"}
        result = extract_bash_fields(tool_input)

        assert result["command_prefix"] is None
        assert result["description"] == "Nothing"

    def test_extract_bash_fields_whitespace_command(self):
        """Test Bash extraction with whitespace-only command."""
        tool_input = {"command": "   ", "description": "Whitespace"}
        result = extract_bash_fields(tool_input)

        assert result["command_prefix"] is None

    def test_extract_webfetch_fields(self):
        """Test WebFetch tool field extraction."""
        tool_input = {
            "url": "https://example.com",
            "prompt": "Extract the title",
        }
        result = extract_webfetch_fields(tool_input)

        assert result["url"] == "https://example.com"
        assert result["prompt"] == "Extract the title"

    def test_extract_websearch_fields(self):
        """Test WebSearch tool field extraction."""
        tool_input = {"query": "python best practices"}
        result = extract_websearch_fields(tool_input)

        assert result["query"] == "python best practices"

    def test_extract_task_fields(self):
        """Test Task tool field extraction."""
        tool_input = {
            "subagent_type": "Explore",
            "description": "Find relevant files",
        }
        result = extract_task_fields(tool_input)

        assert result["subagent_type"] == "Explore"
        assert result["description"] == "Find relevant files"

    def test_extract_tool_fields_unknown_tool(self):
        """Test field extraction for unknown tool type."""
        tool_input = {
            "custom_field": "value",
            "another": "data",
        }
        result = extract_tool_fields("UnknownTool", tool_input)

        # Should return the input as-is
        assert result["custom_field"] == "value"
        assert result["another"] == "data"

    def test_extract_tool_fields_limits_large_strings(self):
        """Test that large string values are truncated for unknown tools."""
        large_string = "x" * 500
        tool_input = {"large_field": large_string}
        result = extract_tool_fields("UnknownTool", tool_input)

        assert len(result["large_field"]) == 203  # 200 + "..."
        assert result["large_field"].endswith("...")


class TestMetricEvent:
    """Tests for MetricEvent dataclass."""

    def test_metric_event_creation(self):
        """Test creating a MetricEvent with required fields."""
        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Read",
            input={"file_path": "/path/to/file.py"},
        )

        assert event.timestamp == "2026-01-05T10:30:00Z"
        assert event.session_id == "sess_abc123"
        assert event.tool == "Read"
        assert event.input == {"file_path": "/path/to/file.py"}
        assert event.output is None
        assert event.issue_id is None
        assert event.worktree is None
        assert event.error is None

    def test_metric_event_with_all_fields(self):
        """Test creating a MetricEvent with all fields."""
        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Edit",
            input={"file_path": "/path/to/file.py", "old_string_len": 10},
            output={"success": True},
            issue_id="bobbin-xyz",
            worktree="/path/to/worktree",
            error=None,
        )

        assert event.issue_id == "bobbin-xyz"
        assert event.worktree == "/path/to/worktree"
        assert event.output == {"success": True}

    def test_metric_event_to_json(self):
        """Test JSON serialization of MetricEvent."""
        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Read",
            input={"file_path": "/path/to/file.py"},
        )

        json_str = event.to_json()
        parsed = json.loads(json_str)

        assert parsed["timestamp"] == "2026-01-05T10:30:00Z"
        assert parsed["session_id"] == "sess_abc123"
        assert parsed["tool"] == "Read"
        assert parsed["input"]["file_path"] == "/path/to/file.py"
        # None values should be excluded
        assert "output" not in parsed
        assert "issue_id" not in parsed

    def test_metric_event_to_json_with_error(self):
        """Test JSON serialization includes error field."""
        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Edit",
            input={"file_path": "/path/to/file.py"},
            error="old_string not found",
        )

        json_str = event.to_json()
        parsed = json.loads(json_str)

        assert parsed["error"] == "old_string not found"


class TestMetricsCollector:
    """Tests for MetricsCollector class."""

    def test_collector_default_path(self, tmp_path):
        """Test that collector uses default path when none provided."""
        with patch.object(Path, "cwd", return_value=tmp_path):
            collector = MetricsCollector()
            assert collector.storage_path == tmp_path / ".tambour" / "metrics.jsonl"

    def test_collector_custom_path(self, tmp_path):
        """Test that collector uses custom path when provided."""
        custom_path = tmp_path / "custom" / "metrics.jsonl"
        collector = MetricsCollector(storage_path=custom_path)
        assert collector.storage_path == custom_path

    def test_store_creates_directory(self, tmp_path):
        """Test that store creates the directory if it doesn't exist."""
        storage_path = tmp_path / "new_dir" / "metrics.jsonl"
        collector = MetricsCollector(storage_path=storage_path)

        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Read",
            input={"file_path": "/path/to/file.py"},
        )

        result = collector.store(event)

        assert result is True
        assert storage_path.exists()
        assert storage_path.parent.exists()

    def test_store_appends_jsonl(self, tmp_path):
        """Test that store appends to JSONL file correctly."""
        storage_path = tmp_path / "metrics.jsonl"
        collector = MetricsCollector(storage_path=storage_path)

        event1 = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Read",
            input={"file_path": "/path/to/file1.py"},
        )
        event2 = MetricEvent(
            timestamp="2026-01-05T10:31:00Z",
            session_id="sess_abc123",
            tool="Write",
            input={"file_path": "/path/to/file2.py"},
        )

        collector.store(event1)
        collector.store(event2)

        lines = storage_path.read_text().strip().split("\n")
        assert len(lines) == 2

        parsed1 = json.loads(lines[0])
        parsed2 = json.loads(lines[1])

        assert parsed1["tool"] == "Read"
        assert parsed2["tool"] == "Write"

    def test_collect_from_env(self, tmp_path):
        """Test collecting metrics from environment variables."""
        storage_path = tmp_path / "metrics.jsonl"
        collector = MetricsCollector(storage_path=storage_path)

        env = {
            "TAMBOUR_EVENT": "tool.used",
            "TAMBOUR_TOOL_NAME": "Read",
            "TAMBOUR_SESSION_ID": "sess_test",
            "TAMBOUR_TIMESTAMP": "2026-01-05T10:30:00Z",
            "TAMBOUR_ISSUE_ID": "bobbin-xyz",
            "TAMBOUR_FILE_PATH": "/path/to/file.py",
        }

        with patch.dict(os.environ, env, clear=False):
            event = collector.collect_from_env()

        assert event is not None
        assert event.tool == "Read"
        assert event.session_id == "sess_test"
        assert event.issue_id == "bobbin-xyz"

    def test_collect_from_env_missing_tool_name(self, tmp_path):
        """Test that collection fails gracefully without tool name."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        env = {
            "TAMBOUR_EVENT": "tool.used",
            "TAMBOUR_SESSION_ID": "sess_test",
        }

        with patch.dict(os.environ, env, clear=False):
            # Clear TAMBOUR_TOOL_NAME if it exists
            with patch.dict(os.environ, {"TAMBOUR_TOOL_NAME": ""}, clear=False):
                event = collector.collect_from_env()

        assert event is None

    def test_collect_from_stdin(self, tmp_path):
        """Test collecting metrics from stdin JSON."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        input_data = json.dumps({
            "event_type": "tool.used",
            "timestamp": "2026-01-05T10:30:00Z",
            "data": {
                "tool_name": "Read",
                "tool_input": {"file_path": "/path/to/file.py"},
                "tool_response": {"success": True},
                "session_id": "sess_stdin",
                "issue_id": "bobbin-123",
            },
        })

        with patch("sys.stdin.read", return_value=input_data):
            event = collector.collect_from_stdin()

        assert event is not None
        assert event.tool == "Read"
        assert event.session_id == "sess_stdin"
        assert event.issue_id == "bobbin-123"

    def test_collect_from_stdin_flat_format(self, tmp_path):
        """Test collecting from stdin with flat JSON format."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        input_data = json.dumps({
            "tool_name": "Edit",
            "tool_input": {"file_path": "/path/to/file.py", "old_string": "foo"},
            "session_id": "sess_flat",
        })

        with patch("sys.stdin.read", return_value=input_data):
            event = collector.collect_from_stdin()

        assert event is not None
        assert event.tool == "Edit"
        assert event.session_id == "sess_flat"

    def test_collect_from_stdin_empty(self, tmp_path):
        """Test that empty stdin returns None."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        with patch("sys.stdin.read", return_value=""):
            event = collector.collect_from_stdin()

        assert event is None

    def test_collect_from_stdin_invalid_json(self, tmp_path):
        """Test that invalid JSON returns None."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        with patch("sys.stdin.read", return_value="not valid json"):
            event = collector.collect_from_stdin()

        assert event is None

    def test_collect_and_store_integration(self, tmp_path):
        """Integration test: collect from env and store."""
        storage_path = tmp_path / "metrics.jsonl"
        collector = MetricsCollector(storage_path=storage_path)

        env = {
            "TAMBOUR_EVENT": "tool.used",
            "TAMBOUR_TOOL_NAME": "Bash",
            "TAMBOUR_SESSION_ID": "sess_integration",
            "TAMBOUR_TIMESTAMP": "2026-01-05T10:30:00Z",
            "TAMBOUR_COMMAND": "cargo build",
            "TAMBOUR_DESCRIPTION": "Build project",
        }

        with patch.dict(os.environ, env, clear=False):
            result = collector.collect_and_store()

        assert result is True
        assert storage_path.exists()

        content = storage_path.read_text()
        parsed = json.loads(content.strip())

        assert parsed["tool"] == "Bash"
        assert parsed["session_id"] == "sess_integration"
        assert parsed["input"]["command_prefix"] == "cargo"
        assert parsed["input"]["description"] == "Build project"


class TestMetricEventFailures:
    """Tests for error handling and edge cases."""

    def test_store_handles_permission_error(self, tmp_path, capsys):
        """Test that store handles permission errors gracefully."""
        storage_path = tmp_path / "readonly" / "metrics.jsonl"
        storage_path.parent.mkdir()
        storage_path.parent.chmod(0o444)  # Read-only directory

        collector = MetricsCollector(storage_path=storage_path)
        event = MetricEvent(
            timestamp="2026-01-05T10:30:00Z",
            session_id="sess_abc123",
            tool="Read",
            input={},
        )

        try:
            result = collector.store(event)
            assert result is False
            captured = capsys.readouterr()
            assert "Error storing metric" in captured.err
        finally:
            # Restore permissions for cleanup
            storage_path.parent.chmod(0o755)

    def test_tool_failed_event(self, tmp_path):
        """Test handling tool.failed events."""
        collector = MetricsCollector(storage_path=tmp_path / "metrics.jsonl")

        input_data = json.dumps({
            "event_type": "tool.failed",
            "data": {
                "tool_name": "Edit",
                "tool_input": {"file_path": "/path/to/file.py"},
                "tool_response": {"error": "old_string not found"},
                "session_id": "sess_failed",
            },
        })

        with patch("sys.stdin.read", return_value=input_data):
            event = collector.collect_from_stdin()

        assert event is not None
        assert event.error == "old_string not found"
