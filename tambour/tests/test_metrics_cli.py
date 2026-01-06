"""Tests for metrics CLI commands."""

from __future__ import annotations

import json
import sys
from argparse import Namespace
from datetime import datetime, timezone, timedelta
from pathlib import Path
from io import StringIO

import pytest

from tambour.metrics.cli import (
    cmd_metrics_show,
    cmd_metrics_hot_files,
    cmd_metrics_file,
    cmd_metrics_session,
    cmd_metrics_complexity,
    cmd_metrics_clear,
    cmd_metrics_refresh,
    format_number,
    format_percent,
    format_duration,
)


def make_event(
    tool: str,
    session_id: str = "sess_test",
    file_path: str | None = None,
    timestamp: str | None = None,
    issue_id: str | None = None,
    success: bool = True,
    error: str | None = None,
) -> dict:
    """Create a test metric event."""
    if timestamp is None:
        timestamp = datetime.now(timezone.utc).isoformat()

    event = {
        "timestamp": timestamp,
        "session_id": session_id,
        "tool": tool,
        "input": {},
        "output": {"success": success},
    }

    if file_path:
        event["input"]["file_path"] = file_path

    if issue_id:
        event["issue_id"] = issue_id

    if error:
        event["error"] = error
        event["output"]["success"] = False

    return event


def create_metrics_file(metrics_path: Path, events: list[dict]) -> None:
    """Create a metrics.jsonl file with the given events."""
    metrics_path.parent.mkdir(parents=True, exist_ok=True)
    with open(metrics_path, "w") as f:
        for event in events:
            f.write(json.dumps(event) + "\n")


class TestFormatHelpers:
    """Tests for formatting helper functions."""

    def test_format_number(self):
        """Test number formatting with thousands separators."""
        assert format_number(0) == "0"
        assert format_number(999) == "999"
        assert format_number(1000) == "1,000"
        assert format_number(1234567) == "1,234,567"

    def test_format_percent(self):
        """Test percentage formatting."""
        assert format_percent(1.0) == "100.0%"
        assert format_percent(0.5) == "50.0%"
        assert format_percent(0.987) == "98.7%"
        assert format_percent(0.0) == "0.0%"

    def test_format_duration(self):
        """Test duration formatting."""
        # No timestamps
        assert format_duration(None, None) == "N/A"
        assert format_duration("2026-01-01T00:00:00Z", None) == "N/A"

        # Seconds
        start = "2026-01-01T00:00:00Z"
        end = "2026-01-01T00:00:30Z"
        assert format_duration(start, end) == "30s"

        # Minutes
        end = "2026-01-01T00:05:00Z"
        assert format_duration(start, end) == "5m"

        # Hours and minutes
        end = "2026-01-01T02:30:00Z"
        assert format_duration(start, end) == "2h 30m"

    def test_format_duration_invalid(self):
        """Test duration formatting with invalid timestamps."""
        assert format_duration("invalid", "invalid") == "N/A"


class TestMetricsShow:
    """Tests for 'metrics show' command."""

    def test_show_empty_metrics(self, tmp_path, capsys):
        """Test show with no metrics file."""
        args = Namespace(
            window=7,
            storage=str(tmp_path / ".tambour" / "metrics.jsonl"),
        )

        result = cmd_metrics_show(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Events collected: 0" in captured.out
        assert "Unique sessions: 0" in captured.out

    def test_show_with_events(self, tmp_path, capsys):
        """Test show with events."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [
            make_event("Read", session_id="sess_1", file_path="/path/file1.py"),
            make_event("Read", session_id="sess_1", file_path="/path/file2.py"),
            make_event("Edit", session_id="sess_2", file_path="/path/file1.py"),
            make_event("Bash", session_id="sess_2"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, storage=str(metrics_path))

        result = cmd_metrics_show(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Events collected: 4" in captured.out
        assert "Unique sessions: 2" in captured.out
        assert "Tool Usage:" in captured.out
        assert "Read" in captured.out

    def test_show_custom_window(self, tmp_path, capsys):
        """Test show with custom time window."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", file_path="/path/file.py")]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=30, storage=str(metrics_path))

        result = cmd_metrics_show(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Last 30 Days" in captured.out


class TestMetricsHotFiles:
    """Tests for 'metrics hot-files' command."""

    def test_hot_files_empty(self, tmp_path, capsys):
        """Test hot-files with no metrics."""
        args = Namespace(
            window=7,
            threshold=5,
            limit=20,
            storage=str(tmp_path / ".tambour" / "metrics.jsonl"),
        )

        result = cmd_metrics_hot_files(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "No files" in captured.out

    def test_hot_files_with_data(self, tmp_path, capsys):
        """Test hot-files with events."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"

        # Create events with different read counts
        events = []
        for _ in range(10):
            events.append(make_event("Read", file_path="/hot/file.py"))
        for _ in range(3):
            events.append(make_event("Read", file_path="/cold/file.py"))

        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=5, limit=20, storage=str(metrics_path))

        result = cmd_metrics_hot_files(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "/hot/file.py" in captured.out
        # Cold file should not appear (below threshold)
        assert "/cold/file.py" not in captured.out

    def test_hot_files_limit(self, tmp_path, capsys):
        """Test hot-files with limit."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"

        events = []
        for i in range(10):
            for _ in range(5):
                events.append(make_event("Read", file_path=f"/file{i}.py"))

        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=1, limit=3, storage=str(metrics_path))

        result = cmd_metrics_hot_files(args)

        assert result == 0
        captured = capsys.readouterr()
        # Should only show 3 files (lines that start with whitespace and have "reads")
        lines = [l for l in captured.out.split("\n") if l.strip().startswith(("1", "2", "3", "4", "5", "6", "7", "8", "9", "0")) and "reads" in l]
        assert len(lines) == 3


class TestMetricsFile:
    """Tests for 'metrics file <path>' command."""

    def test_file_not_found(self, tmp_path, capsys):
        """Test file command with unknown file."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", file_path="/other/file.py")]
        create_metrics_file(metrics_path, events)

        args = Namespace(path="/unknown/file.py", window=7, storage=str(metrics_path))

        result = cmd_metrics_file(args)

        assert result == 1
        captured = capsys.readouterr()
        assert "No metrics found" in captured.err

    def test_file_exact_match(self, tmp_path, capsys):
        """Test file command with exact path match."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [
            make_event("Read", session_id="sess_1", file_path="/path/to/file.py"),
            make_event("Read", session_id="sess_2", file_path="/path/to/file.py"),
            make_event("Edit", session_id="sess_1", file_path="/path/to/file.py"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(path="/path/to/file.py", window=7, storage=str(metrics_path))

        result = cmd_metrics_file(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Metrics for /path/to/file.py" in captured.out
        assert "Total reads: 2" in captured.out
        assert "Unique sessions: 2" in captured.out

    def test_file_partial_match(self, tmp_path, capsys):
        """Test file command with partial path match."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", file_path="/some/long/path/to/file.py")]
        create_metrics_file(metrics_path, events)

        args = Namespace(path="file.py", window=7, storage=str(metrics_path))

        result = cmd_metrics_file(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "/some/long/path/to/file.py" in captured.out

    def test_file_multiple_matches(self, tmp_path, capsys):
        """Test file command with ambiguous match."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [
            make_event("Read", file_path="/path/a/file.py"),
            make_event("Read", file_path="/path/b/file.py"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(path="file.py", window=7, storage=str(metrics_path))

        result = cmd_metrics_file(args)

        assert result == 1
        captured = capsys.readouterr()
        assert "Multiple files match" in captured.err

    def test_file_high_reread_rate(self, tmp_path, capsys):
        """Test file command shows high reread rate indicator."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        # Create 5 reads in a single session = avg 5.0 reads per session
        events = [
            make_event("Read", session_id="sess_1", file_path="/complex/file.py")
            for _ in range(5)
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(path="/complex/file.py", window=7, storage=str(metrics_path))

        result = cmd_metrics_file(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "High re-read rate" in captured.out


class TestMetricsSession:
    """Tests for 'metrics session <session-id>' command."""

    def test_session_not_found(self, tmp_path, capsys):
        """Test session command with unknown session."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", session_id="other_sess")]
        create_metrics_file(metrics_path, events)

        args = Namespace(session_id="unknown", window=7, storage=str(metrics_path))

        result = cmd_metrics_session(args)

        assert result == 1
        captured = capsys.readouterr()
        assert "No session found" in captured.err

    def test_session_exact_match(self, tmp_path, capsys):
        """Test session command with exact session ID."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [
            make_event("Read", session_id="sess_abc123", issue_id="bobbin-xyz"),
            make_event("Edit", session_id="sess_abc123", issue_id="bobbin-xyz"),
            make_event("Bash", session_id="sess_abc123", issue_id="bobbin-xyz"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(session_id="sess_abc123", window=7, storage=str(metrics_path))

        result = cmd_metrics_session(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "sess_abc123" in captured.out
        assert "bobbin-xyz" in captured.out
        assert "Tool uses: 3" in captured.out

    def test_session_prefix_match(self, tmp_path, capsys):
        """Test session command with prefix match."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", session_id="sess_abc123")]
        create_metrics_file(metrics_path, events)

        args = Namespace(session_id="sess_abc", window=7, storage=str(metrics_path))

        result = cmd_metrics_session(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "sess_abc123" in captured.out


class TestMetricsComplexity:
    """Tests for 'metrics complexity' command."""

    def test_complexity_no_signals(self, tmp_path, capsys):
        """Test complexity with no complex files."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [make_event("Read", file_path="/simple/file.py")]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=3.0, storage=str(metrics_path))

        result = cmd_metrics_complexity(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "No files with complexity signals" in captured.out

    def test_complexity_high_reread_rate(self, tmp_path, capsys):
        """Test complexity with high reread rate files."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        # Create 5 reads in a single session = avg 5.0 reads per session
        events = [
            make_event("Read", session_id="sess_1", file_path="/complex/file.py")
            for _ in range(5)
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=3.0, storage=str(metrics_path))

        result = cmd_metrics_complexity(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "/complex/file.py" in captured.out
        assert "High re-read rate" in captured.out

    def test_complexity_very_high_reread_rate(self, tmp_path, capsys):
        """Test complexity with very high reread rate."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        # Create 7 reads in a single session = avg 7.0 reads per session
        events = [
            make_event("Read", session_id="sess_1", file_path="/very/complex/file.py")
            for _ in range(7)
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=3.0, storage=str(metrics_path))

        result = cmd_metrics_complexity(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Very high re-read rate" in captured.out
        assert "Consider refactoring" in captured.out

    def test_complexity_edit_failures(self, tmp_path, capsys):
        """Test complexity with edit failures."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        # Create 5 reads to trigger reread threshold, plus edit failures
        events = [
            make_event("Read", session_id="sess_1", file_path="/failing/file.py")
            for _ in range(5)
        ] + [
            make_event("Edit", file_path="/failing/file.py", success=True),
            make_event("Edit", file_path="/failing/file.py", success=False, error="old_string not found"),
            make_event("Edit", file_path="/failing/file.py", success=False, error="old_string not found"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, threshold=3.0, storage=str(metrics_path))

        result = cmd_metrics_complexity(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "/failing/file.py" in captured.out
        assert "Edit failure rate" in captured.out


class TestMetricsClear:
    """Tests for 'metrics clear' command."""

    def test_clear_no_metrics_file(self, tmp_path, capsys):
        """Test clear with no metrics file."""
        args = Namespace(
            older_than=30,
            dry_run=False,
            storage=str(tmp_path / ".tambour" / "metrics.jsonl"),
        )

        result = cmd_metrics_clear(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "No metrics file found" in captured.out

    def test_clear_dry_run(self, tmp_path, capsys):
        """Test clear with dry-run flag."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"

        now = datetime.now(timezone.utc)
        old = (now - timedelta(days=60)).isoformat()
        recent = (now - timedelta(days=5)).isoformat()

        events = [
            make_event("Read", file_path="/old/file.py", timestamp=old),
            make_event("Read", file_path="/recent/file.py", timestamp=recent),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(older_than=30, dry_run=True, storage=str(metrics_path))

        result = cmd_metrics_clear(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Would remove 1 events" in captured.out
        assert "Would keep 1 events" in captured.out

        # File should not be modified
        with open(metrics_path) as f:
            lines = f.readlines()
        assert len(lines) == 2

    def test_clear_removes_old_events(self, tmp_path, capsys):
        """Test clear actually removes old events."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"

        now = datetime.now(timezone.utc)
        old = (now - timedelta(days=60)).isoformat()
        recent = (now - timedelta(days=5)).isoformat()

        events = [
            make_event("Read", file_path="/old/file.py", timestamp=old),
            make_event("Read", file_path="/recent/file.py", timestamp=recent),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(older_than=30, dry_run=False, storage=str(metrics_path))

        result = cmd_metrics_clear(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Removed 1 events" in captured.out
        assert "Kept 1 events" in captured.out

        # File should be modified
        with open(metrics_path) as f:
            lines = f.readlines()
        assert len(lines) == 1
        assert "/recent/file.py" in lines[0]

    def test_clear_nothing_to_remove(self, tmp_path, capsys):
        """Test clear when no old events exist."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"

        now = datetime.now(timezone.utc)
        recent = (now - timedelta(days=5)).isoformat()

        events = [make_event("Read", file_path="/recent/file.py", timestamp=recent)]
        create_metrics_file(metrics_path, events)

        args = Namespace(older_than=30, dry_run=False, storage=str(metrics_path))

        result = cmd_metrics_clear(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "No events older than" in captured.out

    def test_clear_invalidates_cache(self, tmp_path, capsys):
        """Test clear removes cache file."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        cache_path = tmp_path / ".tambour" / "metrics-agg.json"

        now = datetime.now(timezone.utc)
        old = (now - timedelta(days=60)).isoformat()

        events = [make_event("Read", file_path="/old/file.py", timestamp=old)]
        create_metrics_file(metrics_path, events)

        # Create a dummy cache file
        cache_path.write_text("{}")

        args = Namespace(older_than=30, dry_run=False, storage=str(metrics_path))

        result = cmd_metrics_clear(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Cleared aggregation cache" in captured.out
        assert not cache_path.exists()


class TestMetricsRefresh:
    """Tests for 'metrics refresh' command."""

    def test_refresh_empty(self, tmp_path, capsys):
        """Test refresh with no metrics."""
        args = Namespace(
            window=7,
            storage=str(tmp_path / ".tambour" / "metrics.jsonl"),
        )

        result = cmd_metrics_refresh(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Refreshed aggregations" in captured.out
        assert "Events: 0" in captured.out

    def test_refresh_with_data(self, tmp_path, capsys):
        """Test refresh with events."""
        metrics_path = tmp_path / ".tambour" / "metrics.jsonl"
        events = [
            make_event("Read", session_id="sess_1", file_path="/path/file1.py"),
            make_event("Edit", session_id="sess_2", file_path="/path/file2.py"),
        ]
        create_metrics_file(metrics_path, events)

        args = Namespace(window=7, storage=str(metrics_path))

        result = cmd_metrics_refresh(args)

        assert result == 0
        captured = capsys.readouterr()
        assert "Refreshed aggregations" in captured.out
        assert "Events: 2" in captured.out
        assert "Files: 2" in captured.out
        assert "Sessions: 2" in captured.out


class TestCLIIntegration:
    """Integration tests for the CLI through __main__."""

    def test_metrics_help(self, monkeypatch, capsys):
        """Test metrics --help shows all subcommands."""
        from tambour.__main__ import create_parser

        parser = create_parser()

        # Get help text for metrics command
        with pytest.raises(SystemExit) as exc:
            parser.parse_args(["metrics", "--help"])
        assert exc.value.code == 0

        captured = capsys.readouterr()
        assert "show" in captured.out
        assert "hot-files" in captured.out
        assert "file" in captured.out
        assert "session" in captured.out
        assert "complexity" in captured.out
        assert "clear" in captured.out
        assert "refresh" in captured.out

    def test_parser_defaults(self):
        """Test argument parser defaults."""
        from tambour.__main__ import create_parser

        parser = create_parser()

        args = parser.parse_args(["metrics", "show"])
        assert args.metrics_command == "show"
        assert args.window == 7

        args = parser.parse_args(["metrics", "hot-files"])
        assert args.threshold == 5
        assert args.limit == 20

        args = parser.parse_args(["metrics", "clear"])
        assert args.older_than == 30
        assert args.dry_run is False

    def test_parser_custom_values(self):
        """Test argument parser with custom values."""
        from tambour.__main__ import create_parser

        parser = create_parser()

        args = parser.parse_args(["metrics", "show", "--window", "30"])
        assert args.window == 30

        args = parser.parse_args(["metrics", "hot-files", "--threshold", "10", "--limit", "50"])
        assert args.threshold == 10
        assert args.limit == 50

        args = parser.parse_args(["metrics", "clear", "--older-than", "7", "--dry-run"])
        assert args.older_than == 7
        assert args.dry_run is True
