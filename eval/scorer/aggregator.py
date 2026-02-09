"""Combine all scorer outputs into a unified result.

Aggregates test scoring, diff scoring, and optional LLM judge results
into a single summary dict suitable for report generation.
"""

from __future__ import annotations

from typing import Any


def aggregate(
    test_result: dict[str, Any],
    diff_result: dict[str, Any],
    judge_results: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Aggregate results from all scorers into a single summary.

    Parameters
    ----------
    test_result:
        Output from ``test_scorer.run_tests``.
    diff_result:
        Output from ``diff_scorer.score_diff``.
    judge_results:
        Optional output from ``llm_judge.judge_pairwise``.  May be None
        if the judge was not run for this result.

    Returns a dict with combined metrics.
    """
    combined: dict[str, Any] = {
        "test_passed": test_result.get("passed", False),
        "test_total": test_result.get("total", 0),
        "test_failures": test_result.get("failures", 0),
        "file_precision": diff_result.get("file_precision", 0.0),
        "file_recall": diff_result.get("file_recall", 0.0),
        "f1": diff_result.get("f1", 0.0),
        "exact_file_match": diff_result.get("exact_file_match", False),
    }

    if judge_results:
        combined["judge"] = {
            "overall_winner": judge_results.get("overall_winner"),
            "bias_detected": judge_results.get("bias_detected", False),
            "dimensions": judge_results.get("dimensions", {}),
        }

    return combined


def aggregate_across_runs(results: list[dict[str, Any]]) -> dict[str, Any]:
    """Compute aggregate statistics across multiple run results.

    Parameters
    ----------
    results:
        List of result dicts (as saved by the CLI runner).

    Returns a dict with:
        count, test_pass_rate, avg_file_precision, avg_file_recall,
        avg_f1, avg_duration_seconds, and optional judge_win_rate.
    """
    if not results:
        return {
            "count": 0,
            "test_pass_rate": 0.0,
            "avg_file_precision": 0.0,
            "avg_file_recall": 0.0,
            "avg_f1": 0.0,
            "avg_duration_seconds": 0.0,
        }

    n = len(results)
    test_passes = sum(1 for r in results if r.get("test_result", {}).get("passed"))

    def _avg(key_path: list[str]) -> float:
        vals = []
        for r in results:
            obj = r
            for k in key_path:
                obj = obj.get(k, {}) if isinstance(obj, dict) else {}
            if isinstance(obj, (int, float)):
                vals.append(obj)
        return sum(vals) / len(vals) if vals else 0.0

    stats: dict[str, Any] = {
        "count": n,
        "test_pass_rate": test_passes / n,
        "avg_file_precision": _avg(["diff_result", "file_precision"]),
        "avg_file_recall": _avg(["diff_result", "file_recall"]),
        "avg_f1": _avg(["diff_result", "f1"]),
        "avg_duration_seconds": _avg(["agent_result", "duration_seconds"]),
    }

    # Aggregate judge results if present.
    judge_wins = {"a": 0, "b": 0, "tie": 0}
    judge_count = 0
    for r in results:
        jr = r.get("judge_result")
        if jr and "overall_winner" in jr:
            judge_count += 1
            winner = jr["overall_winner"]
            if winner in judge_wins:
                judge_wins[winner] += 1

    if judge_count > 0:
        stats["judge_summary"] = {
            "count": judge_count,
            "wins": judge_wins,
        }

    return stats
