"""Metrics aggregation pipeline for tambour.

This module derives useful statistics from raw metric events stored in JSONL.
Aggregations are computed on-demand and cached for performance.

Usage:
    from tambour.metrics import aggregator

    # Compute/refresh aggregations (reads metrics.jsonl)
    agg = aggregator.compute(window_days=7)

    # Query aggregations
    hot_files = agg.get_files_by_reads(min_reads=5)
    complex_files = agg.get_files_with_high_reread_rate(threshold=3.0)
    tool_stats = agg.get_tool_stats()
    session_stats = agg.get_session_stats(session_id="abc123")
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Any


@dataclass
class FileStats:
    """Aggregated statistics for a single file.

    Attributes:
        file_path: The path to the file.
        total_reads: Total number of Read operations on this file.
        unique_sessions: Number of distinct sessions that accessed this file.
        avg_reads_per_session: Average reads per session.
        total_edits: Total number of Edit/Write operations.
        edit_success_rate: Proportion of edits that succeeded.
        last_accessed: Most recent access time (ISO format).
        first_accessed: First access time (ISO format).
    """

    file_path: str
    total_reads: int = 0
    unique_sessions: int = 0
    avg_reads_per_session: float = 0.0
    total_edits: int = 0
    edit_success_rate: float = 1.0
    last_accessed: str | None = None
    first_accessed: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {k: v for k, v in asdict(self).items() if v is not None}


@dataclass
class SessionStats:
    """Aggregated statistics for a single session.

    Attributes:
        session_id: The session identifier.
        issue_id: Associated beads issue ID (if available).
        total_tool_uses: Total number of tool invocations.
        unique_files_accessed: Number of distinct files touched.
        read_count: Number of Read operations.
        edit_count: Number of Edit/Write operations.
        edit_success_rate: Proportion of edits that succeeded.
        start_time: Earliest event timestamp (ISO format).
        end_time: Latest event timestamp (ISO format).
    """

    session_id: str
    issue_id: str | None = None
    total_tool_uses: int = 0
    unique_files_accessed: int = 0
    read_count: int = 0
    edit_count: int = 0
    edit_success_rate: float = 1.0
    start_time: str | None = None
    end_time: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {k: v for k, v in asdict(self).items() if v is not None}


@dataclass
class ToolStats:
    """Aggregated statistics for a single tool type.

    Attributes:
        tool: The tool name (Read, Write, Edit, etc.).
        total_uses: Total number of invocations.
        success_count: Number of successful invocations.
        failure_count: Number of failed invocations.
        success_rate: Proportion of successful invocations.
    """

    tool: str
    total_uses: int = 0
    success_count: int = 0
    failure_count: int = 0
    success_rate: float = 1.0

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return asdict(self)


@dataclass
class AggregationResult:
    """Container for all aggregation results.

    Attributes:
        computed_at: When the aggregations were computed (ISO format).
        window_days: The time window used for filtering events.
        event_count: Total number of events processed.
        file_stats: Per-file aggregations, keyed by file path.
        session_stats: Per-session aggregations, keyed by session ID.
        tool_stats: Per-tool aggregations, keyed by tool name.
    """

    computed_at: str
    window_days: int
    event_count: int = 0
    file_stats: dict[str, FileStats] = field(default_factory=dict)
    session_stats: dict[str, SessionStats] = field(default_factory=dict)
    tool_stats: dict[str, ToolStats] = field(default_factory=dict)

    def get_files_by_reads(self, min_reads: int = 1) -> list[FileStats]:
        """Get files sorted by read count.

        Args:
            min_reads: Minimum read count to include.

        Returns:
            List of FileStats sorted by total_reads descending.
        """
        files = [
            stats for stats in self.file_stats.values() if stats.total_reads >= min_reads
        ]
        return sorted(files, key=lambda f: f.total_reads, reverse=True)

    def get_files_with_high_reread_rate(self, threshold: float = 3.0) -> list[FileStats]:
        """Get files with high average re-reads per session.

        Args:
            threshold: Minimum avg_reads_per_session to include.

        Returns:
            List of FileStats sorted by avg_reads_per_session descending.
        """
        files = [
            stats
            for stats in self.file_stats.values()
            if stats.avg_reads_per_session >= threshold
        ]
        return sorted(files, key=lambda f: f.avg_reads_per_session, reverse=True)

    def get_tool_stats(self) -> list[ToolStats]:
        """Get all tool statistics sorted by usage count.

        Returns:
            List of ToolStats sorted by total_uses descending.
        """
        return sorted(
            self.tool_stats.values(), key=lambda t: t.total_uses, reverse=True
        )

    def get_session_stats(self, session_id: str) -> SessionStats | None:
        """Get statistics for a specific session.

        Args:
            session_id: The session identifier.

        Returns:
            SessionStats if found, None otherwise.
        """
        return self.session_stats.get(session_id)

    def get_all_sessions(self) -> list[SessionStats]:
        """Get all session statistics sorted by tool usage.

        Returns:
            List of SessionStats sorted by total_tool_uses descending.
        """
        return sorted(
            self.session_stats.values(), key=lambda s: s.total_tool_uses, reverse=True
        )

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {
            "computed_at": self.computed_at,
            "window_days": self.window_days,
            "event_count": self.event_count,
            "file_stats": {k: v.to_dict() for k, v in self.file_stats.items()},
            "session_stats": {k: v.to_dict() for k, v in self.session_stats.items()},
            "tool_stats": {k: v.to_dict() for k, v in self.tool_stats.items()},
        }

    def to_json(self) -> str:
        """Serialize to JSON string."""
        return json.dumps(self.to_dict(), indent=2)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> AggregationResult:
        """Deserialize from dictionary.

        Args:
            data: Dictionary with aggregation data.

        Returns:
            AggregationResult instance.
        """
        result = cls(
            computed_at=data.get("computed_at", ""),
            window_days=data.get("window_days", 7),
            event_count=data.get("event_count", 0),
        )

        # Reconstruct file stats
        for path, stats_dict in data.get("file_stats", {}).items():
            result.file_stats[path] = FileStats(**stats_dict)

        # Reconstruct session stats
        for session_id, stats_dict in data.get("session_stats", {}).items():
            result.session_stats[session_id] = SessionStats(**stats_dict)

        # Reconstruct tool stats
        for tool_name, stats_dict in data.get("tool_stats", {}).items():
            result.tool_stats[tool_name] = ToolStats(**stats_dict)

        return result


class MetricsAggregator:
    """Aggregates raw metric events into useful statistics.

    Reads events from metrics.jsonl and computes per-file, per-session,
    and per-tool aggregations. Results are cached to metrics-agg.json.
    """

    DEFAULT_METRICS_PATH = ".tambour/metrics.jsonl"
    DEFAULT_CACHE_PATH = ".tambour/metrics-agg.json"
    DEFAULT_WINDOW_DAYS = 7

    def __init__(
        self,
        metrics_path: Path | None = None,
        cache_path: Path | None = None,
    ):
        """Initialize the aggregator.

        Args:
            metrics_path: Path to metrics.jsonl. Defaults to .tambour/metrics.jsonl.
            cache_path: Path to cache file. Defaults to .tambour/metrics-agg.json.
        """
        base_path = Path.cwd()

        if metrics_path is None:
            metrics_path = base_path / self.DEFAULT_METRICS_PATH
        self.metrics_path = Path(metrics_path)

        if cache_path is None:
            cache_path = base_path / self.DEFAULT_CACHE_PATH
        self.cache_path = Path(cache_path)

    def compute(
        self, window_days: int = DEFAULT_WINDOW_DAYS, force: bool = False
    ) -> AggregationResult:
        """Compute aggregations from metrics.jsonl.

        Args:
            window_days: Number of days to include in the time window.
            force: If True, recompute even if cache is fresh.

        Returns:
            AggregationResult with computed statistics.
        """
        # Check cache unless forced
        if not force:
            cached = self._load_cache(window_days)
            if cached is not None:
                return cached

        # Compute fresh aggregations
        result = self._compute_aggregations(window_days)

        # Save to cache
        self._save_cache(result)

        return result

    def _compute_aggregations(self, window_days: int) -> AggregationResult:
        """Compute aggregations from raw events.

        Args:
            window_days: Number of days to include.

        Returns:
            AggregationResult with computed statistics.
        """
        result = AggregationResult(
            computed_at=datetime.now(timezone.utc).isoformat(),
            window_days=window_days,
        )

        # Calculate cutoff time
        cutoff = datetime.now(timezone.utc) - timedelta(days=window_days)

        # Load and process events
        events = self._load_events(cutoff)
        result.event_count = len(events)

        if not events:
            return result

        # Track intermediate data for calculations
        file_sessions: dict[str, set[str]] = {}  # file_path -> set of session_ids
        file_reads: dict[str, int] = {}  # file_path -> read count
        file_edits: dict[str, int] = {}  # file_path -> edit count
        file_edit_successes: dict[str, int] = {}  # file_path -> successful edit count
        file_timestamps: dict[str, list[str]] = {}  # file_path -> list of timestamps

        session_files: dict[str, set[str]] = {}  # session_id -> set of file_paths
        session_issue: dict[str, str] = {}  # session_id -> issue_id
        session_tools: dict[str, int] = {}  # session_id -> tool count
        session_reads: dict[str, int] = {}  # session_id -> read count
        session_edits: dict[str, int] = {}  # session_id -> edit count
        session_edit_successes: dict[str, int] = {}  # session_id -> successful edits
        session_timestamps: dict[str, list[str]] = {}  # session_id -> timestamps

        tool_uses: dict[str, int] = {}  # tool -> total uses
        tool_successes: dict[str, int] = {}  # tool -> successful uses

        for event in events:
            tool = event.get("tool", "")
            session_id = event.get("session_id", "unknown")
            timestamp = event.get("timestamp", "")
            issue_id = event.get("issue_id")
            tool_input = event.get("input", {})
            tool_output = event.get("output", {})
            error = event.get("error")

            # Extract file path for file-based tools
            file_path = tool_input.get("file_path")

            # Determine success/failure
            success = True
            if error:
                success = False
            elif isinstance(tool_output, dict):
                success = tool_output.get("success", True)

            # Track tool stats
            tool_uses[tool] = tool_uses.get(tool, 0) + 1
            if success:
                tool_successes[tool] = tool_successes.get(tool, 0) + 1

            # Track session stats
            session_tools[session_id] = session_tools.get(session_id, 0) + 1
            if issue_id and session_id not in session_issue:
                session_issue[session_id] = issue_id

            if session_id not in session_timestamps:
                session_timestamps[session_id] = []
            session_timestamps[session_id].append(timestamp)

            # Track file-specific stats
            if file_path:
                # File reads
                if tool == "Read":
                    file_reads[file_path] = file_reads.get(file_path, 0) + 1
                    session_reads[session_id] = session_reads.get(session_id, 0) + 1

                # File edits
                if tool in ("Edit", "Write"):
                    file_edits[file_path] = file_edits.get(file_path, 0) + 1
                    session_edits[session_id] = session_edits.get(session_id, 0) + 1
                    if success:
                        file_edit_successes[file_path] = (
                            file_edit_successes.get(file_path, 0) + 1
                        )
                        session_edit_successes[session_id] = (
                            session_edit_successes.get(session_id, 0) + 1
                        )

                # Track file-session associations
                if file_path not in file_sessions:
                    file_sessions[file_path] = set()
                file_sessions[file_path].add(session_id)

                if session_id not in session_files:
                    session_files[session_id] = set()
                session_files[session_id].add(file_path)

                # Track timestamps per file
                if file_path not in file_timestamps:
                    file_timestamps[file_path] = []
                file_timestamps[file_path].append(timestamp)

        # Build file stats
        all_files = set(file_reads.keys()) | set(file_edits.keys())
        for file_path in all_files:
            reads = file_reads.get(file_path, 0)
            edits = file_edits.get(file_path, 0)
            edit_successes = file_edit_successes.get(file_path, 0)
            sessions = file_sessions.get(file_path, set())
            timestamps = sorted(file_timestamps.get(file_path, []))

            unique_sessions = len(sessions)
            avg_reads = reads / unique_sessions if unique_sessions > 0 else 0.0
            success_rate = edit_successes / edits if edits > 0 else 1.0

            result.file_stats[file_path] = FileStats(
                file_path=file_path,
                total_reads=reads,
                unique_sessions=unique_sessions,
                avg_reads_per_session=round(avg_reads, 2),
                total_edits=edits,
                edit_success_rate=round(success_rate, 3),
                first_accessed=timestamps[0] if timestamps else None,
                last_accessed=timestamps[-1] if timestamps else None,
            )

        # Build session stats
        for session_id in session_tools:
            tools = session_tools[session_id]
            files = session_files.get(session_id, set())
            reads = session_reads.get(session_id, 0)
            edits = session_edits.get(session_id, 0)
            edit_successes = session_edit_successes.get(session_id, 0)
            timestamps = sorted(session_timestamps.get(session_id, []))
            issue_id = session_issue.get(session_id)

            success_rate = edit_successes / edits if edits > 0 else 1.0

            result.session_stats[session_id] = SessionStats(
                session_id=session_id,
                issue_id=issue_id,
                total_tool_uses=tools,
                unique_files_accessed=len(files),
                read_count=reads,
                edit_count=edits,
                edit_success_rate=round(success_rate, 3),
                start_time=timestamps[0] if timestamps else None,
                end_time=timestamps[-1] if timestamps else None,
            )

        # Build tool stats
        for tool_name in tool_uses:
            uses = tool_uses[tool_name]
            successes = tool_successes.get(tool_name, 0)
            failures = uses - successes
            success_rate = successes / uses if uses > 0 else 1.0

            result.tool_stats[tool_name] = ToolStats(
                tool=tool_name,
                total_uses=uses,
                success_count=successes,
                failure_count=failures,
                success_rate=round(success_rate, 3),
            )

        return result

    def _load_events(self, cutoff: datetime) -> list[dict[str, Any]]:
        """Load events from metrics.jsonl, filtering by cutoff time.

        Args:
            cutoff: Only include events after this time.

        Returns:
            List of event dictionaries.
        """
        events = []

        if not self.metrics_path.exists():
            return events

        try:
            with open(self.metrics_path) as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue

                    try:
                        event = json.loads(line)
                    except json.JSONDecodeError:
                        continue

                    # Filter by timestamp
                    timestamp_str = event.get("timestamp", "")
                    if timestamp_str:
                        try:
                            # Parse ISO format timestamp
                            ts = datetime.fromisoformat(
                                timestamp_str.replace("Z", "+00:00")
                            )
                            if ts < cutoff:
                                continue
                        except ValueError:
                            # If we can't parse, include the event
                            pass

                    events.append(event)

        except OSError:
            pass

        return events

    def _load_cache(self, window_days: int) -> AggregationResult | None:
        """Load cached aggregations if fresh.

        Args:
            window_days: The requested time window.

        Returns:
            AggregationResult if cache is valid, None otherwise.
        """
        if not self.cache_path.exists():
            return None

        if not self.metrics_path.exists():
            return None

        # Check if cache is stale (metrics file is newer)
        cache_mtime = self.cache_path.stat().st_mtime
        metrics_mtime = self.metrics_path.stat().st_mtime

        if metrics_mtime > cache_mtime:
            return None

        try:
            with open(self.cache_path) as f:
                data = json.load(f)

            # Verify window matches
            if data.get("window_days") != window_days:
                return None

            return AggregationResult.from_dict(data)

        except (json.JSONDecodeError, OSError, KeyError, TypeError):
            return None

    def _save_cache(self, result: AggregationResult) -> bool:
        """Save aggregations to cache file.

        Args:
            result: The aggregation result to cache.

        Returns:
            True if saved successfully, False otherwise.
        """
        try:
            self.cache_path.parent.mkdir(parents=True, exist_ok=True)
            with open(self.cache_path, "w") as f:
                f.write(result.to_json())
            return True
        except OSError:
            return False


def compute(
    window_days: int = MetricsAggregator.DEFAULT_WINDOW_DAYS,
    force: bool = False,
    metrics_path: Path | None = None,
    cache_path: Path | None = None,
) -> AggregationResult:
    """Convenience function to compute aggregations.

    Args:
        window_days: Number of days to include in the time window.
        force: If True, recompute even if cache is fresh.
        metrics_path: Optional path to metrics.jsonl.
        cache_path: Optional path to cache file.

    Returns:
        AggregationResult with computed statistics.
    """
    aggregator = MetricsAggregator(
        metrics_path=metrics_path,
        cache_path=cache_path,
    )
    return aggregator.compute(window_days=window_days, force=force)
