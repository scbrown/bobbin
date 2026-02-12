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
    """Load all JSON result files from the results directory.

    Scans ``results/runs/*/*.json`` (run-based layout) first, then falls
    back to ``results/*.json`` (legacy flat layout).  Skips non-result
    files (e.g. judge_results.json, manifest.json) by checking that each
    file is a dict with a ``task_id`` key.
    """
    results: list[dict[str, Any]] = []

    # Run-based layout.
    runs_dir = results_dir / "runs"
    if runs_dir.is_dir():
        for f in sorted(runs_dir.glob("*/*.json")):
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and "task_id" in data:
                    results.append(data)
            except (json.JSONDecodeError, OSError) as exc:
                logger.warning("Skipping invalid result file %s: %s", f.name, exc)

    # Legacy flat layout.
    for f in sorted(results_dir.glob("*.json")):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            if isinstance(data, dict) and "task_id" in data:
                results.append(data)
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Skipping invalid result file %s: %s", f.name, exc)

    if not results:
        raise ReportError(f"No result JSON files found in {results_dir}")

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

    costs = [
        r["token_usage"]["total_cost_usd"]
        for r in results
        if r.get("token_usage", {}).get("total_cost_usd") is not None
    ]
    input_toks = [
        r["token_usage"]["input_tokens"]
        for r in results
        if r.get("token_usage", {}).get("input_tokens") is not None
    ]
    output_toks = [
        r["token_usage"]["output_tokens"]
        for r in results
        if r.get("token_usage", {}).get("output_tokens") is not None
    ]

    # Tool use metrics (backward compat: missing tool_use_summary → empty).
    tool_calls = [
        r["tool_use_summary"]["total_tool_calls"]
        for r in results
        if r.get("tool_use_summary", {}).get("total_tool_calls") is not None
    ]
    first_edit_turns = [
        r["tool_use_summary"]["first_edit_turn"]
        for r in results
        if r.get("tool_use_summary", {}).get("first_edit_turn") is not None
    ]
    bobbin_cmds = [
        len(r["tool_use_summary"]["bobbin_commands"])
        for r in results
        if r.get("tool_use_summary", {}).get("bobbin_commands") is not None
    ]

    return {
        "count": len(results),
        "test_pass_rate": pass_rate,
        "avg_file_precision": _safe_avg(precisions),
        "avg_file_recall": _safe_avg(recalls),
        "avg_f1": _safe_avg(f1s),
        "avg_duration_seconds": _safe_avg(durations),
        "avg_cost_usd": _safe_avg(costs),
        "avg_input_tokens": _safe_avg(input_toks),
        "avg_output_tokens": _safe_avg(output_toks),
        "has_cost_data": len(costs) > 0,
        "avg_tool_calls": _safe_avg(tool_calls),
        "avg_first_edit_turn": _safe_avg(first_edit_turns),
        "avg_bobbin_commands": _safe_avg(bobbin_cmds),
        "has_tool_use_data": len(tool_calls) > 0,
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

    # Check if any approach has cost data.
    has_cost = any(s.get("has_cost_data") for s in stats.values())

    metrics = [
        ("Runs", "count", False, False),
        ("Test Pass Rate", "test_pass_rate", True, True),
        ("Avg File Precision", "avg_file_precision", True, True),
        ("Avg File Recall", "avg_file_recall", True, True),
        ("Avg F1", "avg_f1", True, True),
        ("Avg Duration (s)", "avg_duration_seconds", False, False),
    ]

    if has_cost:
        metrics.extend([
            ("Avg Cost (USD)", "avg_cost_usd", False, False),
            ("Avg Input Tokens", "avg_input_tokens", False, False),
            ("Avg Output Tokens", "avg_output_tokens", False, False),
        ])

    # Tool use metrics (only when data is available).
    has_tool_use = any(s.get("has_tool_use_data") for s in stats.values())
    if has_tool_use:
        metrics.extend([
            ("Avg Tool Calls", "avg_tool_calls", False, False),
            ("Avg First Edit Turn", "avg_first_edit_turn", False, False),
            ("Avg Bobbin Commands", "avg_bobbin_commands", False, False),
        ])

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
            elif key == "avg_cost_usd":
                row += f" ${val:.2f} |"
            elif key in ("avg_input_tokens", "avg_output_tokens"):
                row += f" {int(val):,} |"
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

    # Check if any result has tool use data.
    has_tool_use = any(
        r.get("tool_use_summary", {}).get("total_tool_calls") is not None
        for r in results
    )

    if has_tool_use:
        lines = [
            "| Task | Approach | Tests Passed | File Precision | File Recall | F1 | Duration | Cost | Tools |",
            "|------|----------|:---:|:---:|:---:|:---:|---:|---:|---:|",
        ]
    else:
        lines = [
            "| Task | Approach | Tests Passed | File Precision | File Recall | F1 | Duration | Cost |",
            "|------|----------|:---:|:---:|:---:|:---:|---:|---:|",
        ]

    for task_id, task_results in by_task.items():
        by_approach = _group_by_approach(task_results)
        for approach, approach_results in by_approach.items():
            stats = _compute_approach_stats(approach_results)
            cost_str = f"${stats['avg_cost_usd']:.2f}" if stats['avg_cost_usd'] else "n/a"
            tools_str = f"{stats['avg_tool_calls']:.0f}" if stats.get('has_tool_use_data') else "n/a"
            row = (
                f"| {task_id} | {approach} "
                f"| {_pct(stats['test_pass_rate'])} "
                f"| {_pct(stats['avg_file_precision'])} "
                f"| {_pct(stats['avg_file_recall'])} "
                f"| {_pct(stats['avg_f1'])} "
                f"| {stats['avg_duration_seconds']:.1f}s "
                f"| {cost_str} |"
            )
            if has_tool_use:
                row += f" {tools_str} |"
            lines.append(row)

    return "\n".join(lines)


def _build_judge_table(judge_results: list[dict]) -> str:
    """Build a summary table from LLM judge pairwise results."""
    # Group by pair type.
    by_pair: dict[str, list[dict]] = {}
    for j in judge_results:
        pair = j.get("pair", "unknown")
        by_pair.setdefault(pair, []).append(j)

    lines = [
        "| Comparison | Tasks | A Wins | B Wins | Ties | Bias Detected |",
        "|------------|:-----:|:------:|:------:|:----:|:-------------:|",
    ]

    for pair_label in sorted(by_pair):
        judgements = by_pair[pair_label]
        n = len(judgements)
        a_wins = sum(1 for j in judgements if j.get("overall_winner") == "a")
        b_wins = sum(1 for j in judgements if j.get("overall_winner") == "b")
        ties = sum(1 for j in judgements if j.get("overall_winner") == "tie")
        bias = sum(1 for j in judgements if j.get("bias_detected"))

        # Get human-readable labels from first result.
        a_label = judgements[0].get("a_label", "A")
        b_label = judgements[0].get("b_label", "B")
        display = f"{a_label} vs {b_label}"

        lines.append(
            f"| {display} | {n} "
            f"| {a_label}: {a_wins} "
            f"| {b_label}: {b_wins} "
            f"| {ties} "
            f"| {bias}/{n} |"
        )

    # Per-task detail table.
    lines.extend([
        "",
        "### Per-Task Judge Detail",
        "",
        "| Task | Comparison | Winner | Consistency | Completeness | Minimality |",
        "|------|------------|--------|:-----------:|:------------:|:----------:|",
    ])

    for j in sorted(judge_results, key=lambda x: (x.get("task_id", ""), x.get("pair", ""))):
        dims = j.get("dimensions", {})
        cons = dims.get("consistency", {})
        comp = dims.get("completeness", {})
        mini = dims.get("minimality", {})
        winner = j.get("named_winner", j.get("overall_winner", "?"))
        lines.append(
            f"| {j.get('task_id', '?')} "
            f"| {j.get('a_label', 'A')} vs {j.get('b_label', 'B')} "
            f"| {winner} "
            f"| {cons.get('a', '?')}/{cons.get('b', '?')} "
            f"| {comp.get('a', '?')}/{comp.get('b', '?')} "
            f"| {mini.get('a', '?')}/{mini.get('b', '?')} |"
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

    # Load judge results if available.
    judge_file = rdir / "judge_results.json"
    judge_results: list[dict] = []
    if judge_file.exists():
        try:
            judge_results = json.loads(judge_file.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Could not load judge results: %s", exc)

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
    ]

    if judge_results:
        sections.extend([
            "## LLM Judge Results",
            "",
            _build_judge_table(judge_results),
            "",
        ])

    sections.extend([
        "## Per-Task Breakdown",
        "",
        _build_per_task_table(results),
        "",
    ])

    report = "\n".join(sections) + "\n"

    # Write output.
    out = Path(output_path)
    try:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(report, encoding="utf-8")
    except OSError as exc:
        raise ReportError(f"Cannot write report to {out}: {exc}") from exc

    logger.info("Report written to %s", out)
