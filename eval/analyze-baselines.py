#!/usr/bin/env python3
"""Analyze baseline eval results for §66 paper.

Generates comparison tables for:
1. No-injection vs with-bobbin baselines
2. Ablation study (one method disabled at a time)
3. Injection quality metrics (precision/recall/F1)

Usage:
    python3 analyze-baselines.py [results_dir]
"""

import json
import glob
import os
import sys
from collections import defaultdict
from pathlib import Path


def load_results(results_dir: str) -> list[dict]:
    """Load all completed result JSONs from runs directories."""
    results = []
    for run_dir in sorted(glob.glob(os.path.join(results_dir, "runs", "*"))):
        for f in glob.glob(os.path.join(run_dir, "*.json")):
            if f.endswith("manifest.json"):
                continue
            try:
                d = json.load(open(f))
                if d.get("status") == "completed":
                    results.append(d)
            except (json.JSONDecodeError, OSError):
                pass
    return results


def group_by(results: list[dict], *keys) -> dict:
    """Group results by one or more keys."""
    groups = defaultdict(list)
    for r in results:
        key = tuple(r.get(k) or "unknown" for k in keys)
        if len(keys) == 1:
            key = key[0]
        groups[key].append(r)
    return dict(groups)


def stats(runs: list[dict]) -> dict:
    """Compute aggregate stats for a set of runs."""
    n = len(runs)
    if n == 0:
        return {}

    passed = sum(1 for r in runs if r.get("test_result", {}).get("passed"))
    f1s = [r.get("diff_result", {}).get("f1", 0) for r in runs]
    precisions = [r.get("diff_result", {}).get("file_precision", 0) for r in runs]
    recalls = [r.get("diff_result", {}).get("file_recall", 0) for r in runs]
    costs = [r.get("agent_result", {}).get("cost_usd", 0) or 0 for r in runs]
    durations = [r.get("agent_result", {}).get("duration_seconds", 0) for r in runs]

    # Injection metrics (with-bobbin only)
    inj_precisions = []
    inj_recalls = []
    inj_f1s = []
    for r in runs:
        ir = r.get("injection_result") or {}
        if ir:
            inj_precisions.append(ir.get("injection_precision", 0))
            inj_recalls.append(ir.get("injection_recall", 0))
            inj_f1s.append(ir.get("injection_f1", 0))

    def avg(lst):
        return sum(lst) / len(lst) if lst else 0

    def std(lst):
        if len(lst) < 2:
            return 0
        m = avg(lst)
        return (sum((x - m) ** 2 for x in lst) / (len(lst) - 1)) ** 0.5

    return {
        "n": n,
        "pass_rate": passed / n,
        "avg_f1": avg(f1s),
        "std_f1": std(f1s),
        "avg_precision": avg(precisions),
        "avg_recall": avg(recalls),
        "avg_cost": avg(costs),
        "avg_duration": avg(durations),
        "inj_precision": avg(inj_precisions) if inj_precisions else None,
        "inj_recall": avg(inj_recalls) if inj_recalls else None,
        "inj_f1": avg(inj_f1s) if inj_f1s else None,
    }


def print_baseline_table(results: list[dict]):
    """Print baseline comparison table (no-bobbin vs with-bobbin)."""
    by_task_approach = group_by(results, "task_id", "approach")

    tasks = sorted(set(r["task_id"] for r in results))
    approaches = ["no-bobbin", "with-bobbin"]

    print("\n## Baseline Comparison: No-Injection vs With-Bobbin\n")
    hdr = f"{'Task':<15} {'Approach':<15} {'N':>3} {'Pass%':>6} {'F1':>6} {'Prec':>6} {'Rec':>6} {'Cost':>8} {'Time':>6}"
    print(hdr)
    print("-" * len(hdr))

    for task in tasks:
        for approach in approaches:
            key = (task, approach)
            runs = by_task_approach.get(key, [])
            if not runs:
                continue
            s = stats(runs)
            print(
                f"{task:<15} {approach:<15} {s['n']:>3} "
                f"{s['pass_rate']*100:>5.0f}% {s['avg_f1']:>5.2f} "
                f"{s['avg_precision']:>5.2f} {s['avg_recall']:>5.2f} "
                f"${s['avg_cost']:>6.2f} {s['avg_duration']:>5.0f}s"
            )
        print()

    # Aggregate across tasks
    print("--- Aggregate ---")
    for approach in approaches:
        runs = [r for r in results if r.get("approach") == approach]
        if not runs:
            continue
        s = stats(runs)
        print(
            f"{'ALL':<15} {approach:<15} {s['n']:>3} "
            f"{s['pass_rate']*100:>5.0f}% {s['avg_f1']:>5.2f} "
            f"{s['avg_precision']:>5.2f} {s['avg_recall']:>5.2f} "
            f"${s['avg_cost']:>6.2f} {s['avg_duration']:>5.0f}s"
        )


def print_ablation_table(results: list[dict]):
    """Print ablation study table."""
    ablation_runs = [
        r for r in results
        if r.get("approach", "").startswith("with-bobbin+")
    ]
    if not ablation_runs:
        print("\n## Ablation Study\n(No ablation results found)")
        return

    by_approach = group_by(ablation_runs, "approach")
    baseline_runs = [r for r in results if r.get("approach") == "with-bobbin"]
    baseline_stats = stats(baseline_runs) if baseline_runs else {}

    print("\n## Ablation Study: Effect of Disabling Individual Methods\n")
    if baseline_stats:
        print(
            f"Baseline (with-bobbin): Pass={baseline_stats['pass_rate']*100:.0f}%, "
            f"F1={baseline_stats['avg_f1']:.2f}, Cost=${baseline_stats['avg_cost']:.2f}\n"
        )

    hdr = f"{'Disabled Method':<35} {'N':>3} {'Pass%':>6} {'F1':>6} {'Delta':>7} {'Cost':>8}"
    print(hdr)
    print("-" * len(hdr))

    for approach in sorted(by_approach.keys()):
        runs = by_approach[approach]
        s = stats(runs)
        method = approach.replace("with-bobbin+", "")
        delta = s["avg_f1"] - baseline_stats.get("avg_f1", 0) if baseline_stats else 0
        delta_str = f"{delta:>+6.2f}" if baseline_stats else "  N/A"
        print(
            f"{method:<35} {s['n']:>3} "
            f"{s['pass_rate']*100:>5.0f}% {s['avg_f1']:>5.2f} "
            f"{delta_str} ${s['avg_cost']:>6.2f}"
        )


def print_injection_table(results: list[dict]):
    """Print injection quality metrics table."""
    bobbin_runs = [
        r for r in results
        if r.get("injection_result") is not None
    ]
    if not bobbin_runs:
        print("\n## Injection Quality Metrics\n(No injection results found)")
        return

    by_task = group_by(bobbin_runs, "task_id")

    print("\n## Injection Quality Metrics (Bobbin Approaches Only)\n")
    hdr = f"{'Task':<15} {'N':>3} {'Inj Prec':>9} {'Inj Rec':>9} {'Inj F1':>8}"
    print(hdr)
    print("-" * len(hdr))

    for task in sorted(by_task.keys()):
        runs = by_task[task]
        s = stats(runs)
        if s.get("inj_precision") is not None:
            print(
                f"{task:<15} {s['n']:>3} "
                f"{s['inj_precision']:>8.2f} {s['inj_recall']:>8.2f} "
                f"{s['inj_f1']:>7.2f}"
            )

    # Aggregate
    s = stats(bobbin_runs)
    if s.get("inj_precision") is not None:
        print(f"\n{'ALL':<15} {s['n']:>3} "
              f"{s['inj_precision']:>8.2f} {s['inj_recall']:>8.2f} "
              f"{s['inj_f1']:>7.2f}")


def main():
    results_dir = sys.argv[1] if len(sys.argv) > 1 else "results"
    results = load_results(results_dir)

    if not results:
        print(f"No completed results found in {results_dir}/runs/")
        sys.exit(1)

    print(f"Loaded {len(results)} completed results from {results_dir}/runs/")

    print_baseline_table(results)
    print_ablation_table(results)
    print_injection_table(results)


if __name__ == "__main__":
    main()
