"""Generate markdown summary report from eval results."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class ReportError(Exception):
    """Raised when report generation fails."""


def _load_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load all JSON result files from the results directory."""
    json_files = sorted(results_dir.glob("*.json"))
    if not json_files:
        raise ReportError(f"No result JSON files found in {results_dir}")

    results = []
    for f in json_files:
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            results.append(data)
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Skipping invalid result file %s: %s", f.name, exc)
    return results


def _group_by_task(results: list[dict]) -> dict[str, list[dict]]:
    """Group results by task_id."""
    groups: dict[str, list[dict]] = {}
    for r in results:
        tid = r.get("task_id", "unknown")
        groups.setdefault(tid, []).append(r)
    return dict(sorted(groups.items()))


def _group_by_approach(results: list[dict]) -> dict[str, list[dict]]:
    """Group results by approach (no-bobbin / with-bobbin)."""
    groups: dict[str, list[dict]] = {}
    for r in results:
        approach = r.get("approach", "unknown")
        groups.setdefault(approach, []).append(r)
    return dict(sorted(groups.items()))


def _safe_avg(values: list[float | int]) -> float:
    """Compute average, returning 0.0 for empty lists."""
    if not values:
        return 0.0
    return sum(values) / len(values)


def _pct(value: float) -> str:
    """Format a float as percentage string."""
    return f"{value * 100:.1f}%"


def _compute_approach_stats(results: list[dict]) -> dict[str, Any]:
    """Compute aggregate statistics for a set of results from one approach."""
    test_passes = [r for r in results if r.get("test_result", {}).get("passed")]
    pass_rate = len(test_passes) / len(results) if results else 0.0

    precisions = [
        r["diff_result"]["file_precision"]
        for r in results
        if r.get("diff_result", {}).get("file_precision") is not None
    ]
    recalls = [
        r["diff_result"]["file_recall"]
        for r in results
        if r.get("diff_result", {}).get("file_recall") is not None
    ]
    f1s = [
        r["diff_result"]["f1"]
        for r in results
        if r.get("diff_result", {}).get("f1") is not None
    ]

    durations = [
        r["agent_result"]["duration_seconds"]
        for r in results
        if r.get("agent_result", {}).get("duration_seconds") is not None
    ]

    return {
        "count": len(results),
        "test_pass_rate": pass_rate,
        "avg_file_precision": _safe_avg(precisions),
        "avg_file_recall": _safe_avg(recalls),
        "avg_f1": _safe_avg(f1s),
        "avg_duration_seconds": _safe_avg(durations),
    }


def _format_delta(baseline: float, treatment: float, is_pct: bool = False) -> str:
    """Format a delta between two values."""
    if baseline == 0:
        return "n/a"
    diff = treatment - baseline
    if is_pct:
        return f"{diff:+.1%}"
    pct_change = diff / baseline
    return f"{pct_change:+.0%}"


def _build_summary_table(
    stats: dict[str, dict[str, Any]],
) -> str:
    """Build the main comparison table."""
    approaches = sorted(stats.keys())

    lines = []
    header = "| Metric |"
    sep = "|--------|"
    for a in approaches:
        header += f" {a} |"
        sep += ":-:|"

    if len(approaches) == 2:
        header += " Delta |"
        sep += ":-:|"

    lines.append(header)
    lines.append(sep)

    metrics = [
        ("Runs", "count", False, False),
        ("Test Pass Rate", "test_pass_rate", True, True),
        ("Avg File Precision", "avg_file_precision", True, True),
        ("Avg File Recall", "avg_file_recall", True, True),
        ("Avg F1", "avg_f1", True, True),
        ("Avg Duration (s)", "avg_duration_seconds", False, False),
    ]

    for label, key, is_pct, show_as_pct in metrics:
        row = f"| {label} |"
        values = []
        for a in approaches:
            val = stats[a].get(key, 0)
            values.append(val)
            if is_pct and show_as_pct:
                row += f" {_pct(val)} |"
            elif key == "count":
                row += f" {int(val)} |"
            else:
                row += f" {val:.1f} |"

        if len(approaches) == 2:
            delta = _format_delta(values[0], values[1], is_pct=is_pct)
            row += f" {delta} |"

        lines.append(row)

    return "\n".join(lines)


def _build_per_task_table(
    results: list[dict],
) -> str:
    """Build a per-task breakdown table."""
    by_task = _group_by_task(results)

    lines = [
        "| Task | Approach | Tests Passed | File Precision | File Recall | F1 | Duration |",
        "|------|----------|:---:|:---:|:---:|:---:|---:|",
    ]

    for task_id, task_results in by_task.items():
        by_approach = _group_by_approach(task_results)
        for approach, approach_results in by_approach.items():
            stats = _compute_approach_stats(approach_results)
            lines.append(
                f"| {task_id} | {approach} "
                f"| {_pct(stats['test_pass_rate'])} "
                f"| {_pct(stats['avg_file_precision'])} "
                f"| {_pct(stats['avg_file_recall'])} "
                f"| {_pct(stats['avg_f1'])} "
                f"| {stats['avg_duration_seconds']:.1f}s |"
            )

    return "\n".join(lines)


def generate_report(results_dir: str, output_path: str) -> None:
    """Read results JSON files and generate a markdown summary report.

    Each JSON file in *results_dir* should contain a single run result with keys:
        task_id, approach, attempt, agent_result, test_result, diff_result

    The report includes:
    - Summary comparison table (with vs without bobbin)
    - Per-task breakdown
    - Configuration details

    Parameters
    ----------
    results_dir:
        Directory containing JSON result files.
    output_path:
        Path where the markdown report will be written.

    Raises :class:`ReportError` if no results are found or output cannot be written.
    """
    rdir = Path(results_dir)
    if not rdir.is_dir():
        raise ReportError(f"Results directory not found: {rdir}")

    results = _load_results(rdir)
    if not results:
        raise ReportError(f"No valid results loaded from {rdir}")

    logger.info("Generating report from %d results", len(results))

    # Compute per-approach stats.
    by_approach = _group_by_approach(results)
    stats = {approach: _compute_approach_stats(runs) for approach, runs in by_approach.items()}

    # Build report sections.
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    total_tasks = len(_group_by_task(results))

    sections = [
        "# Bobbin Eval Report",
        "",
        f"Generated: {timestamp}",
        f"Total tasks: {total_tasks} | Total runs: {len(results)}",
        "",
        "## Summary",
        "",
        _build_summary_table(stats),
        "",
        "## Per-Task Breakdown",
        "",
        _build_per_task_table(results),
        "",
    ]

    report = "\n".join(sections) + "\n"

    # Write output.
    out = Path(output_path)
    try:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(report, encoding="utf-8")
    except OSError as exc:
        raise ReportError(f"Cannot write report to {out}: {exc}") from exc

    logger.info("Report written to %s", out)
