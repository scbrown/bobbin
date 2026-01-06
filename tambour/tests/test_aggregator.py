"""Tests for metrics aggregation pipeline."""

from __future__ import annotations

import json
import time
from datetime import datetime, timezone, timedelta
from pathlib import Path

import pytest

from tambour.metrics.aggregator import (
    AggregationResult,
    FileStats,
    MetricsAggregator,
    SessionStats,
    ToolStats,
    compute,
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


class TestFileStats:
    """Tests for FileStats dataclass."""

    def test_creation_defaults(self):
        """Test FileStats with default values."""
        stats = FileStats(file_path="/path/to/file.py")
        assert stats.file_path == "/path/to/file.py"
        assert stats.total_reads == 0
        assert stats.unique_sessions == 0
        assert stats.avg_reads_per_session == 0.0
        assert stats.total_edits == 0
        assert stats.edit_success_rate == 1.0

    def test_creation_with_values(self):
        """Test FileStats with all values."""
        stats = FileStats(
            file_path="/path/to/file.py",
            total_reads=10,
            unique_sessions=3,
            avg_reads_per_session=3.33,
            total_edits=5,
            edit_success_rate=0.8,
            first_accessed="2026-01-01T00:00:00Z",
            last_accessed="2026-01-05T12:00:00Z",
        )
        assert stats.total_reads == 10
        assert stats.unique_sessions == 3
        assert stats.edit_success_rate == 0.8

    def test_to_dict(self):
        """Test FileStats to_dict excludes None values."""
        stats = FileStats(
            file_path="/path/to/file.py",
            total_reads=5,
        )
        d = stats.to_dict()
        assert "file_path" in d
        assert d["total_reads"] == 5
        assert "first_accessed" not in d or d["first_accessed"] is None


class TestSessionStats:
    """Tests for SessionStats dataclass."""

    def test_creation_defaults(self):
        """Test SessionStats with default values."""
        stats = SessionStats(session_id="sess_123")
        assert stats.session_id == "sess_123"
        assert stats.total_tool_uses == 0
        assert stats.unique_files_accessed == 0

    def test_creation_with_values(self):
        """Test SessionStats with all values."""
        stats = SessionStats(
            session_id="sess_123",
            issue_id="bobbin-xyz",
            total_tool_uses=100,
            unique_files_accessed=25,
            read_count=50,
            edit_count=10,
            edit_success_rate=0.9,
            start_time="2026-01-05T08:00:00Z",
            end_time="2026-01-05T10:00:00Z",
        )
        assert stats.issue_id == "bobbin-xyz"
        assert stats.total_tool_uses == 100


class TestToolStats:
    """Tests for ToolStats dataclass."""

    def test_creation(self):
        """Test ToolStats creation."""
        stats = ToolStats(
            tool="Read",
            total_uses=100,
            success_count=95,
            failure_count=5,
            success_rate=0.95,
        )
        assert stats.tool == "Read"
        assert stats.total_uses == 100
        assert stats.success_rate == 0.95


class TestAggregationResult:
    """Tests for AggregationResult dataclass."""

    def test_creation(self):
        """Test AggregationResult creation."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
            event_count=100,
        )
        assert result.computed_at == "2026-01-05T12:00:00Z"
        assert result.window_days == 7
        assert result.event_count == 100
        assert len(result.file_stats) == 0

    def test_get_files_by_reads(self):
        """Test filtering and sorting files by read count."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
        )
        result.file_stats["file1.py"] = FileStats(file_path="file1.py", total_reads=10)
        result.file_stats["file2.py"] = FileStats(file_path="file2.py", total_reads=2)
        result.file_stats["file3.py"] = FileStats(file_path="file3.py", total_reads=20)

        # Get all files
        files = result.get_files_by_reads(min_reads=1)
        assert len(files) == 3
        assert files[0].file_path == "file3.py"  # Highest reads first
        assert files[1].file_path == "file1.py"
        assert files[2].file_path == "file2.py"

        # Filter by min_reads
        files = result.get_files_by_reads(min_reads=5)
        assert len(files) == 2
        assert all(f.total_reads >= 5 for f in files)

    def test_get_files_with_high_reread_rate(self):
        """Test filtering files by reread rate."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
        )
        result.file_stats["simple.py"] = FileStats(
            file_path="simple.py", avg_reads_per_session=1.0
        )
        result.file_stats["complex.py"] = FileStats(
            file_path="complex.py", avg_reads_per_session=5.0
        )
        result.file_stats["medium.py"] = FileStats(
            file_path="medium.py", avg_reads_per_session=3.0
        )

        files = result.get_files_with_high_reread_rate(threshold=3.0)
        assert len(files) == 2
        assert files[0].file_path == "complex.py"
        assert files[1].file_path == "medium.py"

    def test_get_tool_stats(self):
        """Test getting tool stats sorted by usage."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
        )
        result.tool_stats["Edit"] = ToolStats(tool="Edit", total_uses=50)
        result.tool_stats["Read"] = ToolStats(tool="Read", total_uses=200)
        result.tool_stats["Bash"] = ToolStats(tool="Bash", total_uses=30)

        tools = result.get_tool_stats()
        assert len(tools) == 3
        assert tools[0].tool == "Read"
        assert tools[1].tool == "Edit"
        assert tools[2].tool == "Bash"

    def test_get_session_stats(self):
        """Test getting specific session stats."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
        )
        result.session_stats["sess_1"] = SessionStats(
            session_id="sess_1", total_tool_uses=50
        )
        result.session_stats["sess_2"] = SessionStats(
            session_id="sess_2", total_tool_uses=100
        )

        stats = result.get_session_stats("sess_1")
        assert stats is not None
        assert stats.total_tool_uses == 50

        stats = result.get_session_stats("nonexistent")
        assert stats is None

    def test_get_all_sessions(self):
        """Test getting all sessions sorted by tool usage."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
        )
        result.session_stats["sess_1"] = SessionStats(
            session_id="sess_1", total_tool_uses=50
        )
        result.session_stats["sess_2"] = SessionStats(
            session_id="sess_2", total_tool_uses=100
        )
        result.session_stats["sess_3"] = SessionStats(
            session_id="sess_3", total_tool_uses=25
        )

        sessions = result.get_all_sessions()
        assert len(sessions) == 3
        assert sessions[0].session_id == "sess_2"
        assert sessions[1].session_id == "sess_1"
        assert sessions[2].session_id == "sess_3"

    def test_to_dict_and_from_dict_roundtrip(self):
        """Test serialization/deserialization roundtrip."""
        original = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
            event_count=100,
        )
        original.file_stats["file.py"] = FileStats(
            file_path="file.py",
            total_reads=10,
            unique_sessions=3,
        )
        original.session_stats["sess_1"] = SessionStats(
            session_id="sess_1",
            issue_id="bobbin-xyz",
            total_tool_uses=50,
        )
        original.tool_stats["Read"] = ToolStats(
            tool="Read",
            total_uses=100,
            success_count=98,
            failure_count=2,
            success_rate=0.98,
        )

        # Round-trip through dict
        d = original.to_dict()
        restored = AggregationResult.from_dict(d)

        assert restored.computed_at == original.computed_at
        assert restored.window_days == original.window_days
        assert restored.event_count == original.event_count
        assert len(restored.file_stats) == 1
        assert restored.file_stats["file.py"].total_reads == 10
        assert len(restored.session_stats) == 1
        assert restored.session_stats["sess_1"].issue_id == "bobbin-xyz"
        assert len(restored.tool_stats) == 1
        assert restored.tool_stats["Read"].success_rate == 0.98

    def test_to_json(self):
        """Test JSON serialization."""
        result = AggregationResult(
            computed_at="2026-01-05T12:00:00Z",
            window_days=7,
            event_count=10,
        )

        json_str = result.to_json()
        parsed = json.loads(json_str)

        assert parsed["computed_at"] == "2026-01-05T12:00:00Z"
        assert parsed["window_days"] == 7


class TestMetricsAggregator:
    """Tests for MetricsAggregator class."""

    def test_default_paths(self, tmp_path, monkeypatch):
        """Test default path resolution."""
        monkeypatch.chdir(tmp_path)
        aggregator = MetricsAggregator()
        assert aggregator.metrics_path == tmp_path / ".tambour" / "metrics.jsonl"
        assert aggregator.cache_path == tmp_path / ".tambour" / "metrics-agg.json"

    def test_custom_paths(self, tmp_path):
        """Test custom path configuration."""
        metrics = tmp_path / "custom" / "metrics.jsonl"
        cache = tmp_path / "custom" / "cache.json"
        aggregator = MetricsAggregator(metrics_path=metrics, cache_path=cache)
        assert aggregator.metrics_path == metrics
        assert aggregator.cache_path == cache

    def test_compute_empty_metrics(self, tmp_path):
        """Test aggregation with no metrics file."""
        aggregator = MetricsAggregator(
            metrics_path=tmp_path / "nonexistent.jsonl",
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        assert result.event_count == 0
        assert len(result.file_stats) == 0
        assert len(result.session_stats) == 0
        assert len(result.tool_stats) == 0

    def test_compute_with_events(self, tmp_path):
        """Test aggregation with events."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        # Create test events
        events = [
            make_event("Read", session_id="sess_1", file_path="/path/file1.py"),
            make_event("Read", session_id="sess_1", file_path="/path/file1.py"),
            make_event("Read", session_id="sess_2", file_path="/path/file1.py"),
            make_event("Edit", session_id="sess_1", file_path="/path/file2.py"),
            make_event("Bash", session_id="sess_1"),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        result = aggregator.compute(window_days=7)

        # Verify event count
        assert result.event_count == 5

        # Verify file stats
        assert "/path/file1.py" in result.file_stats
        file1 = result.file_stats["/path/file1.py"]
        assert file1.total_reads == 3
        assert file1.unique_sessions == 2
        assert file1.avg_reads_per_session == 1.5  # 3 reads / 2 sessions

        assert "/path/file2.py" in result.file_stats
        file2 = result.file_stats["/path/file2.py"]
        assert file2.total_edits == 1
        assert file2.total_reads == 0

        # Verify session stats
        assert "sess_1" in result.session_stats
        sess1 = result.session_stats["sess_1"]
        assert sess1.total_tool_uses == 4
        assert sess1.read_count == 2
        assert sess1.edit_count == 1
        assert sess1.unique_files_accessed == 2

        assert "sess_2" in result.session_stats
        sess2 = result.session_stats["sess_2"]
        assert sess2.total_tool_uses == 1
        assert sess2.read_count == 1

        # Verify tool stats
        assert "Read" in result.tool_stats
        assert result.tool_stats["Read"].total_uses == 3
        assert "Edit" in result.tool_stats
        assert result.tool_stats["Edit"].total_uses == 1
        assert "Bash" in result.tool_stats
        assert result.tool_stats["Bash"].total_uses == 1

    def test_compute_edit_success_rate(self, tmp_path):
        """Test edit success rate calculation."""
        metrics_path = tmp_path / "metrics.jsonl"

        events = [
            make_event("Edit", file_path="/path/file.py", success=True),
            make_event("Edit", file_path="/path/file.py", success=True),
            make_event("Edit", file_path="/path/file.py", success=False, error="old_string not found"),
            make_event("Edit", file_path="/path/file.py", success=True),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        file_stats = result.file_stats["/path/file.py"]
        assert file_stats.total_edits == 4
        assert file_stats.edit_success_rate == 0.75  # 3/4 successful

        tool_stats = result.tool_stats["Edit"]
        assert tool_stats.total_uses == 4
        assert tool_stats.success_count == 3
        assert tool_stats.failure_count == 1
        assert tool_stats.success_rate == 0.75

    def test_compute_time_window_filtering(self, tmp_path):
        """Test that old events are filtered out."""
        metrics_path = tmp_path / "metrics.jsonl"

        now = datetime.now(timezone.utc)
        old_timestamp = (now - timedelta(days=10)).isoformat()
        recent_timestamp = (now - timedelta(days=3)).isoformat()

        events = [
            make_event("Read", file_path="/old/file.py", timestamp=old_timestamp),
            make_event("Read", file_path="/recent/file.py", timestamp=recent_timestamp),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        # Only recent event should be included
        assert result.event_count == 1
        assert "/recent/file.py" in result.file_stats
        assert "/old/file.py" not in result.file_stats

    def test_compute_issue_id_tracking(self, tmp_path):
        """Test that issue_id is tracked in session stats."""
        metrics_path = tmp_path / "metrics.jsonl"

        events = [
            make_event("Read", session_id="sess_1", issue_id="bobbin-xyz"),
            make_event("Edit", session_id="sess_1", issue_id="bobbin-xyz"),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        assert result.session_stats["sess_1"].issue_id == "bobbin-xyz"

    def test_caching_saves_result(self, tmp_path):
        """Test that aggregations are cached."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [make_event("Read", file_path="/path/file.py")]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        # First compute should create cache
        result1 = aggregator.compute(window_days=7)
        assert cache_path.exists()

        # Cache should contain valid data
        with open(cache_path) as f:
            cached = json.load(f)
        assert cached["window_days"] == 7
        assert cached["event_count"] == 1

    def test_caching_uses_cache_when_fresh(self, tmp_path):
        """Test that cache is used when metrics haven't changed."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [make_event("Read", file_path="/path/file.py")]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        # First compute
        result1 = aggregator.compute(window_days=7)
        computed_at1 = result1.computed_at

        # Small delay to ensure different timestamp if recomputed
        time.sleep(0.01)

        # Second compute should use cache (same computed_at)
        result2 = aggregator.compute(window_days=7)
        assert result2.computed_at == computed_at1

    def test_caching_invalidates_when_metrics_newer(self, tmp_path):
        """Test that cache is invalidated when metrics file is newer."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [make_event("Read", file_path="/path/file1.py")]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        # First compute
        result1 = aggregator.compute(window_days=7)
        computed_at1 = result1.computed_at

        # Small delay to ensure file mtime changes
        time.sleep(0.01)

        # Add new event (modifies metrics file)
        with open(metrics_path, "a") as f:
            f.write(json.dumps(make_event("Read", file_path="/path/file2.py")) + "\n")

        # Second compute should recompute (different computed_at)
        result2 = aggregator.compute(window_days=7)
        assert result2.computed_at != computed_at1
        assert result2.event_count == 2

    def test_caching_invalidates_on_window_change(self, tmp_path):
        """Test that cache is invalidated when window_days changes."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [make_event("Read", file_path="/path/file.py")]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        # First compute with 7 days
        result1 = aggregator.compute(window_days=7)
        computed_at1 = result1.computed_at

        time.sleep(0.01)

        # Second compute with different window should recompute
        result2 = aggregator.compute(window_days=14)
        assert result2.window_days == 14
        # computed_at will be different since we recomputed

    def test_force_recompute(self, tmp_path):
        """Test that force=True bypasses cache."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [make_event("Read", file_path="/path/file.py")]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        # First compute
        result1 = aggregator.compute(window_days=7)
        computed_at1 = result1.computed_at

        time.sleep(0.01)

        # Force recompute
        result2 = aggregator.compute(window_days=7, force=True)
        assert result2.computed_at != computed_at1

    def test_handles_malformed_events(self, tmp_path):
        """Test that malformed events are skipped."""
        metrics_path = tmp_path / "metrics.jsonl"

        with open(metrics_path, "w") as f:
            f.write("not valid json\n")
            f.write('{"tool": "Read", "session_id": "sess_1"}\n')  # Valid
            f.write("{incomplete json\n")
            f.write('{"tool": "Edit", "session_id": "sess_2"}\n')  # Valid

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        # Only valid events should be counted
        assert result.event_count == 2

    def test_handles_empty_lines(self, tmp_path):
        """Test that empty lines are skipped."""
        metrics_path = tmp_path / "metrics.jsonl"

        with open(metrics_path, "w") as f:
            f.write('{"tool": "Read", "session_id": "sess_1"}\n')
            f.write("\n")
            f.write("   \n")
            f.write('{"tool": "Edit", "session_id": "sess_2"}\n')

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)
        assert result.event_count == 2

    def test_timestamp_formats(self, tmp_path):
        """Test handling of different timestamp formats."""
        metrics_path = tmp_path / "metrics.jsonl"

        now = datetime.now(timezone.utc)
        recent = now - timedelta(days=1)

        events = [
            # ISO format with Z
            {"tool": "Read", "session_id": "s1", "timestamp": recent.strftime("%Y-%m-%dT%H:%M:%SZ")},
            # ISO format with +00:00
            {"tool": "Edit", "session_id": "s2", "timestamp": recent.strftime("%Y-%m-%dT%H:%M:%S+00:00")},
            # ISO format with microseconds
            {"tool": "Bash", "session_id": "s3", "timestamp": recent.isoformat()},
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)
        assert result.event_count == 3


class TestComputeFunction:
    """Tests for the compute() convenience function."""

    def test_compute_function(self, tmp_path):
        """Test the compute() convenience function."""
        metrics_path = tmp_path / "metrics.jsonl"
        cache_path = tmp_path / "cache.json"

        events = [
            make_event("Read", file_path="/path/file.py"),
            make_event("Edit", file_path="/path/file.py"),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        result = compute(
            window_days=7,
            metrics_path=metrics_path,
            cache_path=cache_path,
        )

        assert result.event_count == 2
        assert len(result.tool_stats) == 2


class TestEdgeCases:
    """Tests for edge cases and special scenarios."""

    def test_single_event(self, tmp_path):
        """Test aggregation with single event."""
        metrics_path = tmp_path / "metrics.jsonl"

        with open(metrics_path, "w") as f:
            f.write(json.dumps(make_event("Read", file_path="/single.py")) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        assert result.event_count == 1
        assert len(result.file_stats) == 1
        assert result.file_stats["/single.py"].total_reads == 1
        assert result.file_stats["/single.py"].avg_reads_per_session == 1.0

    def test_all_events_same_file(self, tmp_path):
        """Test aggregation when all events are for the same file."""
        metrics_path = tmp_path / "metrics.jsonl"

        events = [
            make_event("Read", session_id=f"sess_{i}", file_path="/same.py")
            for i in range(10)
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        assert len(result.file_stats) == 1
        assert result.file_stats["/same.py"].total_reads == 10
        assert result.file_stats["/same.py"].unique_sessions == 10
        assert result.file_stats["/same.py"].avg_reads_per_session == 1.0

    def test_events_without_file_path(self, tmp_path):
        """Test that events without file_path are handled."""
        metrics_path = tmp_path / "metrics.jsonl"

        events = [
            make_event("Bash"),  # No file_path
            make_event("WebSearch"),  # No file_path
            make_event("Read", file_path="/path/file.py"),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        # Only one file should be tracked
        assert len(result.file_stats) == 1
        # But all events should be counted for tools
        assert len(result.tool_stats) == 3

    def test_session_timestamps(self, tmp_path):
        """Test that session start/end times are captured correctly."""
        metrics_path = tmp_path / "metrics.jsonl"

        base = datetime.now(timezone.utc)
        events = [
            make_event(
                "Read",
                session_id="sess_1",
                timestamp=(base - timedelta(hours=2)).isoformat(),
            ),
            make_event(
                "Edit",
                session_id="sess_1",
                timestamp=(base - timedelta(hours=1)).isoformat(),
            ),
            make_event(
                "Read",
                session_id="sess_1",
                timestamp=base.isoformat(),
            ),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        sess = result.session_stats["sess_1"]
        assert sess.start_time is not None
        assert sess.end_time is not None
        # Start should be earlier than end
        assert sess.start_time < sess.end_time

    def test_no_edits_success_rate(self, tmp_path):
        """Test success rate when there are no edits."""
        metrics_path = tmp_path / "metrics.jsonl"

        events = [
            make_event("Read", file_path="/path/file.py"),
            make_event("Read", file_path="/path/file.py"),
        ]

        with open(metrics_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        result = aggregator.compute(window_days=7)

        # No edits means success rate defaults to 1.0
        assert result.file_stats["/path/file.py"].edit_success_rate == 1.0
        assert result.session_stats["sess_test"].edit_success_rate == 1.0


class TestPerformance:
    """Performance tests with larger datasets."""

    def test_large_metrics_file(self, tmp_path):
        """Test aggregation with 10k+ events."""
        metrics_path = tmp_path / "metrics.jsonl"

        # Generate 10k events
        with open(metrics_path, "w") as f:
            for i in range(10000):
                event = make_event(
                    tool=["Read", "Edit", "Bash"][i % 3],
                    session_id=f"sess_{i % 100}",  # 100 unique sessions
                    file_path=f"/path/file_{i % 500}.py",  # 500 unique files
                )
                f.write(json.dumps(event) + "\n")

        aggregator = MetricsAggregator(
            metrics_path=metrics_path,
            cache_path=tmp_path / "cache.json",
        )

        # This should complete in reasonable time
        import time

        start = time.time()
        result = aggregator.compute(window_days=30)  # Wider window to include all
        elapsed = time.time() - start

        assert result.event_count == 10000
        assert len(result.file_stats) == 500
        assert len(result.session_stats) == 100
        assert len(result.tool_stats) == 3

        # Should complete in under 5 seconds (generous for CI)
        assert elapsed < 5.0, f"Aggregation took {elapsed:.2f}s"
