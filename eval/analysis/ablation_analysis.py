"""Ablation analysis for the context injection paper (aegis-o1jqap).

Produces:
1. Ablation impact table: delta from baseline for each method disabled
2. Per-task ablation breakdown: consistency across tasks
3. Injection usage metrics: how well injected context predicted touched files
4. Cost/duration analysis

Usage:
    python -m eval.analysis.ablation_analysis eval/results
"""

from __future__ import annotations

import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any


def load_completed_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load all completed result JSONs."""
    results = []
    runs_dir = results_dir / "runs"
    if not runs_dir.is_dir():
        return results
    for run_dir in sorted(runs_dir.iterdir()):
        if not run_dir.is_dir() or run_dir.name.startswith("_"):
            continue
        for f in sorted(run_dir.glob("*.json")):
            if f.name == "manifest.json" or f.name.endswith("_metrics.jsonl"):
                continue
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and data.get("status") == "completed":
                    results.append(data)
            except (json.JSONDecodeError, OSError):
                continue
    return results


def group_by_task_approach(
    results: list[dict],
) -> dict[tuple[str, str], list[dict]]:
    """Group results by (task_id, approach)."""
    groups: dict[tuple[str, str], list[dict]] = defaultdict(list)
    for r in results:
        key = (r["task_id"], r.get("approach", "unknown"))
        groups[key].append(r)
    return groups


def avg(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def stdev(values: list[float]) -> float:
    return statistics.stdev(values) if len(values) > 1 else 0.0


def extract_metrics(runs: list[dict]) -> dict[str, Any]:
    """Extract summary metrics from a list of runs."""
    f1s = [r.get("diff_result", {}).get("f1", 0) for r in runs]
    pass_rates = [1 if r.get("test_result", {}).get("passed") else 0 for r in runs]
    costs = [r.get("agent_result", {}).get("cost_usd", 0) or 0 for r in runs]
    durations = [r.get("agent_result", {}).get("duration_seconds", 0) or 0 for r in runs]

    # Injection metrics (with-bobbin only)
    inj_f1s = [
        r.get("injection_result", {}).get("injection_f1", 0)
        for r in runs
        if r.get("injection_result")
    ]

    return {
        "n": len(runs),
        "f1_mean": avg(f1s),
        "f1_std": stdev(f1s),
        "pass_rate": avg(pass_rates),
        "cost_mean": avg(costs),
        "duration_mean": avg(durations),
        "injection_f1_mean": avg(inj_f1s) if inj_f1s else None,
        "f1_values": f1s,
    }


def print_ablation_impact(
    groups: dict[tuple[str, str], list[dict]],
    study_tasks: list[str],
) -> None:
    """Print ablation impact table: each method's contribution."""
    ablation_approaches = [
        ("with-bobbin+semantic_weight=0.0", "Semantic search"),
        ("with-bobbin+coupling_depth=0", "Coupling expansion"),
        ("with-bobbin+recency_weight=0.0", "Recency signal"),
        ("with-bobbin+doc_demotion=0.0", "Doc demotion"),
        ("with-bobbin+gate_threshold=1.0", "Quality gate"),
        ("with-bobbin+blame_bridging=false", "Blame bridging"),
    ]

    print("## Ablation Impact Summary")
    print()
    print("Effect of disabling each method (averaged across study tasks):")
    print()
    print(
        f"| Method Disabled | Baseline F1 | Ablated F1 | Delta | Impact | N |"
    )
    print("|-----------------|:-----------:|:----------:|:-----:|:------:|:-:|")

    for approach, label in ablation_approaches:
        baseline_f1s = []
        ablated_f1s = []

        for task in study_tasks:
            bl_runs = groups.get((task, "with-bobbin"), [])
            ab_runs = groups.get((task, approach), [])
            if bl_runs and ab_runs:
                baseline_f1s.append(avg([r.get("diff_result", {}).get("f1", 0) for r in bl_runs]))
                ablated_f1s.append(avg([r.get("diff_result", {}).get("f1", 0) for r in ab_runs]))

        if baseline_f1s and ablated_f1s:
            bl = avg(baseline_f1s)
            ab = avg(ablated_f1s)
            delta = ab - bl
            impact = "hurts" if delta < -0.02 else ("helps" if delta > 0.02 else "neutral")
            n = len(baseline_f1s)
            print(f"| {label:<15} | {bl:.3f} | {ab:.3f} | {delta:+.3f} | {impact} | {n} |")
        else:
            print(f"| {label:<15} | — | — | — | no data | 0 |")

    print()


def print_per_task_ablation(
    groups: dict[tuple[str, str], list[dict]],
    study_tasks: list[str],
) -> None:
    """Print per-task ablation breakdown for consistency analysis."""
    approaches = [
        "no-bobbin",
        "with-bobbin",
        "with-bobbin+semantic_weight=0.0",
        "with-bobbin+coupling_depth=0",
        "with-bobbin+recency_weight=0.0",
        "with-bobbin+doc_demotion=0.0",
        "with-bobbin+gate_threshold=1.0",
        "with-bobbin+blame_bridging=false",
    ]

    print("## Per-Task Ablation Breakdown")
    print()
    print("| Task | Approach | N | F1 (mean±std) | Pass% | Cost |")
    print("|------|----------|:-:|:-------------:|:-----:|:----:|")

    for task in study_tasks:
        for approach in approaches:
            runs = groups.get((task, approach), [])
            if runs:
                m = extract_metrics(runs)
                f1_str = f"{m['f1_mean']:.3f}±{m['f1_std']:.3f}"
                print(
                    f"| {task} | {approach} | {m['n']} | {f1_str} | {m['pass_rate']*100:.0f}% | ${m['cost_mean']:.2f} |"
                )
            else:
                print(f"| {task} | {approach} | 0 | — | — | — |")
        print(f"| | | | | | |")


def print_injection_analysis(
    groups: dict[tuple[str, str], list[dict]],
    study_tasks: list[str],
) -> None:
    """Print injection usage analysis: did agents use what bobbin gave them?"""
    print("## Injection Usage Analysis")
    print()
    print("How well bobbin's injected files predicted what the agent actually touched:")
    print()
    print("| Task | Approach | Injection Precision | Injection Recall | Injection F1 |")
    print("|------|----------|:-------------------:|:----------------:|:------------:|")

    wb_approaches = [
        "with-bobbin",
        "with-bobbin+semantic_weight=0.0",
        "with-bobbin+coupling_depth=0",
        "with-bobbin+recency_weight=0.0",
        "with-bobbin+doc_demotion=0.0",
        "with-bobbin+gate_threshold=1.0",
        "with-bobbin+blame_bridging=false",
    ]

    for task in study_tasks:
        for approach in wb_approaches:
            runs = groups.get((task, approach), [])
            inj_runs = [r for r in runs if r.get("injection_result")]
            if inj_runs:
                prec = avg([r["injection_result"].get("injection_precision", 0) for r in inj_runs])
                rec = avg([r["injection_result"].get("injection_recall", 0) for r in inj_runs])
                f1 = avg([r["injection_result"].get("injection_f1", 0) for r in inj_runs])
                print(f"| {task} | {approach} | {prec:.3f} | {rec:.3f} | {f1:.3f} |")


def print_baseline_comparison(
    groups: dict[tuple[str, str], list[dict]],
    study_tasks: list[str],
) -> None:
    """Print no-bobbin vs with-bobbin baseline comparison."""
    print("## Baseline Comparison: No-Injection vs With-Injection")
    print()
    print("| Task | No-Bobbin F1 | With-Bobbin F1 | Delta | Tests (NB) | Tests (WB) |")
    print("|------|:------------:|:--------------:|:-----:|:----------:|:----------:|")

    total_nb_f1 = []
    total_wb_f1 = []

    for task in study_tasks:
        nb = groups.get((task, "no-bobbin"), [])
        wb = groups.get((task, "with-bobbin"), [])
        if nb:
            nb_f1 = avg([r.get("diff_result", {}).get("f1", 0) for r in nb])
            nb_pass = avg([1 if r.get("test_result", {}).get("passed") else 0 for r in nb])
            total_nb_f1.append(nb_f1)
        else:
            nb_f1 = None
            nb_pass = None
        if wb:
            wb_f1 = avg([r.get("diff_result", {}).get("f1", 0) for r in wb])
            wb_pass = avg([1 if r.get("test_result", {}).get("passed") else 0 for r in wb])
            total_wb_f1.append(wb_f1)
        else:
            wb_f1 = None
            wb_pass = None

        nb_str = f"{nb_f1:.3f}" if nb_f1 is not None else "—"
        wb_str = f"{wb_f1:.3f}" if wb_f1 is not None else "—"
        delta = f"{wb_f1 - nb_f1:+.3f}" if nb_f1 is not None and wb_f1 is not None else "—"
        nb_p = f"{nb_pass*100:.0f}%" if nb_pass is not None else "—"
        wb_p = f"{wb_pass*100:.0f}%" if wb_pass is not None else "—"
        print(f"| {task} | {nb_str} | {wb_str} | {delta} | {nb_p} | {wb_p} |")

    if total_nb_f1 and total_wb_f1:
        m_nb = avg(total_nb_f1)
        m_wb = avg(total_wb_f1)
        print(f"| **Average** | **{m_nb:.3f}** | **{m_wb:.3f}** | **{m_wb - m_nb:+.3f}** | | |")
    print()


def main() -> None:
    if len(sys.argv) < 2:
        print("Usage: python -m eval.analysis.ablation_analysis <results_dir>", file=sys.stderr)
        sys.exit(1)

    results_dir = Path(sys.argv[1])
    results = load_completed_results(results_dir)
    if not results:
        print("No completed results found.", file=sys.stderr)
        sys.exit(1)

    groups = group_by_task_approach(results)

    # Study tasks (from run-baseline-study.sh)
    study_tasks = ["ruff-001", "cargo-001", "django-001", "pandas-001"]

    # Also include any tasks that have data
    all_tasks = sorted({r["task_id"] for r in results})
    extra_tasks = [t for t in all_tasks if t not in study_tasks]

    print(f"# Ablation Analysis — Context Injection Paper")
    print(f"")
    print(f"Generated from {len(results)} completed results across {len(all_tasks)} tasks.")
    print(f"Study tasks: {', '.join(study_tasks)}")
    if extra_tasks:
        print(f"Additional tasks with data: {', '.join(extra_tasks)}")
    print()

    print_baseline_comparison(groups, study_tasks)
    print_ablation_impact(groups, study_tasks)
    print_per_task_ablation(groups, study_tasks)
    print_injection_analysis(groups, study_tasks)


if __name__ == "__main__":
    main()
