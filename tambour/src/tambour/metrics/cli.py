"""CLI commands for metrics queries.

This module provides the command handlers for the metrics subcommands:
- show: Display summary of collected metrics
- hot-files: List files by read count
- file: Show details for a specific file
- session: Show details for a specific session
- complexity: Show files with complexity signals
- clear: Remove old events from metrics.jsonl
- refresh: Force refresh of cached aggregations
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import argparse


def format_number(n: int) -> str:
    """Format a number with thousands separators."""
    return f"{n:,}"


def format_percent(rate: float) -> str:
    """Format a rate as a percentage."""
    return f"{rate * 100:.1f}%"


def format_duration(start_iso: str | None, end_iso: str | None) -> str:
    """Format duration between two ISO timestamps."""
    if not start_iso or not end_iso:
        return "N/A"

    try:
        start = datetime.fromisoformat(start_iso.replace("Z", "+00:00"))
        end = datetime.fromisoformat(end_iso.replace("Z", "+00:00"))
        delta = end - start

        total_seconds = int(delta.total_seconds())
        if total_seconds < 60:
            return f"{total_seconds}s"
        elif total_seconds < 3600:
            minutes = total_seconds // 60
            return f"{minutes}m"
        else:
            hours = total_seconds // 3600
            minutes = (total_seconds % 3600) // 60
            return f"{hours}h {minutes}m"
    except (ValueError, TypeError):
        return "N/A"


def cmd_metrics_show(args: argparse.Namespace) -> int:
    """Handle 'metrics show' command - display summary."""
    from tambour.metrics import compute

    window = getattr(args, "window", 7)
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, metrics_path=metrics_path)
    except Exception as e:
        print(f"Error computing metrics: {e}", file=sys.stderr)
        return 1

    # Header
    print(f"=== Tambour Metrics Summary (Last {window} Days) ===")
    print(f"Events collected: {format_number(agg.event_count)}")
    print(f"Unique sessions: {len(agg.session_stats)}")
    print(f"Unique files: {len(agg.file_stats)}")
    print()

    # Tool usage
    tool_stats = agg.get_tool_stats()
    if tool_stats:
        print("Tool Usage:")
        # Find max tool name length for alignment
        max_name_len = max(len(t.tool) for t in tool_stats)
        for tool in tool_stats:
            name = tool.tool.ljust(max_name_len)
            count = format_number(tool.total_uses).rjust(6)
            rate = format_percent(tool.success_rate)
            print(f"  {name}  {count} ({rate} success)")
    else:
        print("No tool usage data available.")

    return 0


def cmd_metrics_hot_files(args: argparse.Namespace) -> int:
    """Handle 'metrics hot-files' command - list files by read count."""
    from tambour.metrics import compute

    window = getattr(args, "window", 7)
    threshold = getattr(args, "threshold", 5)
    limit = getattr(args, "limit", 20)
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, metrics_path=metrics_path)
    except Exception as e:
        print(f"Error computing metrics: {e}", file=sys.stderr)
        return 1

    hot_files = agg.get_files_by_reads(min_reads=threshold)

    if not hot_files:
        print(f"No files with ≥{threshold} reads in the last {window} days.")
        return 0

    print(f"=== Hot Files (≥{threshold} reads in {window} days) ===")

    # Apply limit
    if limit > 0:
        hot_files = hot_files[:limit]

    for file_stats in hot_files:
        reads = str(file_stats.total_reads).rjust(4)
        print(f"  {reads} reads  {file_stats.file_path}")

    return 0


def cmd_metrics_file(args: argparse.Namespace) -> int:
    """Handle 'metrics file <path>' command - show file details."""
    from tambour.metrics import compute

    file_path = args.path
    window = getattr(args, "window", 7)
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, metrics_path=metrics_path)
    except Exception as e:
        print(f"Error computing metrics: {e}", file=sys.stderr)
        return 1

    # Try to find the file - exact match or partial match
    file_stats = agg.file_stats.get(file_path)

    if not file_stats:
        # Try partial match
        matches = [
            stats for path, stats in agg.file_stats.items()
            if file_path in path or path.endswith(file_path)
        ]
        if len(matches) == 1:
            file_stats = matches[0]
        elif len(matches) > 1:
            print(f"Multiple files match '{file_path}':", file=sys.stderr)
            for m in matches[:10]:
                print(f"  {m.file_path}", file=sys.stderr)
            return 1

    if not file_stats:
        print(f"No metrics found for: {file_path}", file=sys.stderr)
        return 1

    print(f"=== Metrics for {file_stats.file_path} ===")
    print(f"Total reads: {file_stats.total_reads}")
    print(f"Unique sessions: {file_stats.unique_sessions}")

    if file_stats.unique_sessions > 0:
        reread_note = ""
        if file_stats.avg_reads_per_session >= 3.0:
            reread_note = "  ← High re-read rate"
        print(f"Average reads per session: {file_stats.avg_reads_per_session:.2f}{reread_note}")

    if file_stats.total_edits > 0:
        print(f"Total edits: {file_stats.total_edits}")
        print(f"Edit success rate: {format_percent(file_stats.edit_success_rate)}")

    if file_stats.last_accessed:
        # Format timestamp nicely
        try:
            ts = datetime.fromisoformat(file_stats.last_accessed.replace("Z", "+00:00"))
            formatted = ts.strftime("%Y-%m-%d %H:%M")
        except (ValueError, TypeError):
            formatted = file_stats.last_accessed
        print(f"Last accessed: {formatted}")

    if file_stats.first_accessed:
        try:
            ts = datetime.fromisoformat(file_stats.first_accessed.replace("Z", "+00:00"))
            formatted = ts.strftime("%Y-%m-%d %H:%M")
        except (ValueError, TypeError):
            formatted = file_stats.first_accessed
        print(f"First accessed: {formatted}")

    return 0


def cmd_metrics_session(args: argparse.Namespace) -> int:
    """Handle 'metrics session <session-id>' command - show session details."""
    from tambour.metrics import compute

    session_id = args.session_id
    window = getattr(args, "window", 7)
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, metrics_path=metrics_path)
    except Exception as e:
        print(f"Error computing metrics: {e}", file=sys.stderr)
        return 1

    # Try to find the session - exact match or prefix match
    session_stats = agg.get_session_stats(session_id)

    if not session_stats:
        # Try prefix match
        matches = [
            stats for sid, stats in agg.session_stats.items()
            if sid.startswith(session_id)
        ]
        if len(matches) == 1:
            session_stats = matches[0]
        elif len(matches) > 1:
            print(f"Multiple sessions match '{session_id}':", file=sys.stderr)
            for m in matches[:10]:
                print(f"  {m.session_id}", file=sys.stderr)
            return 1

    if not session_stats:
        print(f"No session found: {session_id}", file=sys.stderr)
        return 1

    # Header with issue ID if available
    if session_stats.issue_id:
        print(f"=== Session {session_stats.session_id} ({session_stats.issue_id}) ===")
    else:
        print(f"=== Session {session_stats.session_id} ===")

    # Duration
    duration = format_duration(session_stats.start_time, session_stats.end_time)
    print(f"Duration: {duration}")

    print(f"Tool uses: {session_stats.total_tool_uses}")
    print(f"Files accessed: {session_stats.unique_files_accessed}")
    print(f"Read operations: {session_stats.read_count}")

    if session_stats.edit_count > 0:
        print(f"Edit operations: {session_stats.edit_count} ({format_percent(session_stats.edit_success_rate)} success)")

    return 0


def cmd_metrics_complexity(args: argparse.Namespace) -> int:
    """Handle 'metrics complexity' command - show complexity warnings."""
    from tambour.metrics import compute

    window = getattr(args, "window", 7)
    reread_threshold = getattr(args, "threshold", 3.0)
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, metrics_path=metrics_path)
    except Exception as e:
        print(f"Error computing metrics: {e}", file=sys.stderr)
        return 1

    # Find files with complexity signals
    complex_files = []

    for file_path, stats in agg.file_stats.items():
        signals = []
        recommendations = []

        # High re-read rate
        if stats.avg_reads_per_session >= reread_threshold:
            if stats.avg_reads_per_session >= 6.0:
                signals.append(f"Very high re-read rate: {stats.avg_reads_per_session:.1f} per session")
                recommendations.append("Consider refactoring or splitting")
            else:
                signals.append(f"High re-read rate: {stats.avg_reads_per_session:.1f} per session (threshold: {reread_threshold})")
                recommendations.append("Add documentation")

        # Edit failure rate
        if stats.total_edits > 0 and stats.edit_success_rate < 0.9:
            failure_rate = (1 - stats.edit_success_rate) * 100
            signals.append(f"Edit failure rate: {failure_rate:.0f}%")

        if signals:
            complex_files.append((file_path, stats, signals, recommendations))

    if not complex_files:
        print(f"No files with complexity signals in the last {window} days.")
        return 0

    print("=== Files with Complexity Signals ===")

    for file_path, stats, signals, recommendations in complex_files:
        print(f"{file_path}")
        for signal in signals:
            print(f"  - {signal}")
        for rec in recommendations:
            print(f"  - Recommendation: {rec}")
        print()

    return 0


def cmd_metrics_clear(args: argparse.Namespace) -> int:
    """Handle 'metrics clear' command - remove old events."""
    from tambour.metrics.collector import MetricsCollector

    older_than = getattr(args, "older_than", 30)
    dry_run = getattr(args, "dry_run", False)
    storage = getattr(args, "storage", None)

    if storage:
        metrics_path = Path(storage)
    else:
        metrics_path = Path.cwd() / MetricsCollector.DEFAULT_METRICS_PATH

    if not metrics_path.exists():
        print(f"No metrics file found at: {metrics_path}")
        return 0

    # Calculate cutoff
    cutoff = datetime.now(timezone.utc) - timedelta(days=older_than)
    cutoff_str = cutoff.isoformat()

    # Read and filter events
    kept_events = []
    removed_count = 0

    try:
        with open(metrics_path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue

                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    # Keep malformed lines (or drop them?)
                    kept_events.append(line)
                    continue

                timestamp_str = event.get("timestamp", "")
                if timestamp_str:
                    try:
                        ts = datetime.fromisoformat(timestamp_str.replace("Z", "+00:00"))
                        if ts < cutoff:
                            removed_count += 1
                            continue
                    except ValueError:
                        pass

                kept_events.append(line)
    except OSError as e:
        print(f"Error reading metrics file: {e}", file=sys.stderr)
        return 1

    if dry_run:
        print(f"Would remove {removed_count} events older than {older_than} days")
        print(f"Would keep {len(kept_events)} events")
        return 0

    if removed_count == 0:
        print(f"No events older than {older_than} days to remove")
        return 0

    # Write back filtered events
    try:
        with open(metrics_path, "w") as f:
            for line in kept_events:
                f.write(line + "\n")

        print(f"Removed {removed_count} events older than {older_than} days")
        print(f"Kept {len(kept_events)} events")

        # Invalidate cache by removing it
        cache_path = metrics_path.parent / "metrics-agg.json"
        if cache_path.exists():
            cache_path.unlink()
            print("Cleared aggregation cache")

        return 0
    except OSError as e:
        print(f"Error writing metrics file: {e}", file=sys.stderr)
        return 1


def cmd_metrics_refresh(args: argparse.Namespace) -> int:
    """Handle 'metrics refresh' command - force refresh aggregations."""
    from tambour.metrics import compute

    window = getattr(args, "window", 7)
    force = getattr(args, "force", False) or True  # Always force for refresh command
    storage = getattr(args, "storage", None)
    metrics_path = Path(storage) if storage else None

    try:
        agg = compute(window_days=window, force=force, metrics_path=metrics_path)
        print(f"Refreshed aggregations for {window}-day window")
        print(f"  Events: {format_number(agg.event_count)}")
        print(f"  Files: {len(agg.file_stats)}")
        print(f"  Sessions: {len(agg.session_stats)}")
        print(f"  Tools: {len(agg.tool_stats)}")
        return 0
    except Exception as e:
        print(f"Error refreshing metrics: {e}", file=sys.stderr)
        return 1
