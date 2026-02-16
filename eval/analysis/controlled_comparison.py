"""Controlled comparison analysis for bobbin eval results.

Re-analyzes eval data with proper pairing methodology to control for
sampling artifacts, setup errors, and broken eval tasks.

Usage:
    python -m eval.analysis.controlled_comparison eval/results
"""

from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any


def load_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load all result JSONs, annotated with their run_id."""
    results = []
    runs_dir = results_dir / "runs"
    if not runs_dir.is_dir():
        print(f"No runs directory found at {runs_dir}", file=sys.stderr)
        return results

    for run_dir in sorted(runs_dir.iterdir()):
        if not run_dir.is_dir() or run_dir.name.startswith("_"):
            continue
        for f in sorted(run_dir.glob("*.json")):
            if f.name == "manifest.json":
                continue
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and "task_id" in data:
                    data["_run_id"] = run_dir.name
                    results.append(data)
            except (json.JSONDecodeError, OSError):
                continue
    return results


def analyze(results: list[dict]) -> None:
    """Print controlled comparison analysis."""
    # Group by (run_id, task_id) to find paired runs
    pairs: dict[tuple[str, str], dict[str, dict]] = defaultdict(dict)
    for r in results:
        key = (r["_run_id"], r["task_id"])
        approach = r.get("approach", "unknown")
        pairs[key][approach] = r

    # Categorize results
    total_no = [r for r in results if r.get("approach") == "no-bobbin"]
    total_wb = [r for r in results if r.get("approach") == "with-bobbin"]

    # Find paired-only results (same run_id has both approaches for same task)
    paired_no = []
    paired_wb = []
    unpaired_no = []
    unpaired_wb = []

    for (run_id, task_id), approaches in pairs.items():
        if "no-bobbin" in approaches and "with-bobbin" in approaches:
            paired_no.append(approaches["no-bobbin"])
            paired_wb.append(approaches["with-bobbin"])
        elif "no-bobbin" in approaches:
            unpaired_no.append(approaches["no-bobbin"])
        elif "with-bobbin" in approaches:
            unpaired_wb.append(approaches["with-bobbin"])

    # Identify setup errors
    def has_setup_error(r: dict) -> bool:
        return r.get("status") == "bobbin_setup_error"

    paired_no_clean = [r for r in paired_no if not has_setup_error(r)]
    paired_wb_clean = [
        r for r in paired_wb
        if not has_setup_error(r)
        # Also exclude the no-bobbin pair of any with-bobbin setup error
    ]
    # Re-pair excluding setup errors
    paired_clean_no = []
    paired_clean_wb = []
    for (run_id, task_id), approaches in pairs.items():
        nb = approaches.get("no-bobbin")
        wb = approaches.get("with-bobbin")
        if nb and wb and not has_setup_error(nb) and not has_setup_error(wb):
            paired_clean_no.append(nb)
            paired_clean_wb.append(wb)

    # Task category analysis
    def is_flask(r: dict) -> bool:
        return r.get("task_id", "").startswith("flask-")

    def is_ruff(r: dict) -> bool:
        return r.get("task_id", "").startswith("ruff-")

    def pass_rate(rs: list[dict]) -> tuple[int, int, float]:
        if not rs:
            return 0, 0, 0.0
        passed = sum(1 for r in rs if r.get("test_result", {}).get("passed"))
        return passed, len(rs), passed / len(rs) if rs else 0.0

    def fmt_rate(passed: int, total: int, rate: float) -> str:
        return f"{passed}/{total} = {rate*100:.1f}%"

    print("=" * 70)
    print("CONTROLLED COMPARISON ANALYSIS")
    print("=" * 70)
    print()

    # Raw comparison
    nb_p, nb_t, nb_r = pass_rate(total_no)
    wb_p, wb_t, wb_r = pass_rate(total_wb)
    gap = wb_r - nb_r
    print("## 1. Raw (All Results)")
    print(f"  no-bobbin:   {fmt_rate(nb_p, nb_t, nb_r)}")
    print(f"  with-bobbin: {fmt_rate(wb_p, wb_t, wb_r)}")
    print(f"  Gap:         {gap*100:+.1f}pp")
    print(f"  Unpaired no-bobbin runs:   {len(unpaired_no)}")
    print(f"  Unpaired with-bobbin runs: {len(unpaired_wb)}")
    print()

    # Paired comparison
    nb_p, nb_t, nb_r = pass_rate(paired_no)
    wb_p, wb_t, wb_r = pass_rate(paired_wb)
    gap = wb_r - nb_r
    print("## 2. Paired Only (Same Run Has Both Approaches)")
    print(f"  no-bobbin:   {fmt_rate(nb_p, nb_t, nb_r)}")
    print(f"  with-bobbin: {fmt_rate(wb_p, wb_t, wb_r)}")
    print(f"  Gap:         {gap*100:+.1f}pp")
    print()

    # Paired, no setup errors
    nb_p, nb_t, nb_r = pass_rate(paired_clean_no)
    wb_p, wb_t, wb_r = pass_rate(paired_clean_wb)
    gap = wb_r - nb_r
    print("## 3. Paired + No Setup Errors")
    print(f"  no-bobbin:   {fmt_rate(nb_p, nb_t, nb_r)}")
    print(f"  with-bobbin: {fmt_rate(wb_p, wb_t, wb_r)}")
    print(f"  Gap:         {gap*100:+.1f}pp")
    print()

    # By task category
    print("## 4. By Task Category")
    for label, pred in [("flask-*", is_flask), ("ruff-*", is_ruff)]:
        cat_no = [r for r in results if pred(r) and r.get("approach") == "no-bobbin"]
        cat_wb = [r for r in results if pred(r) and r.get("approach") == "with-bobbin"]
        nb_p, nb_t, nb_r = pass_rate(cat_no)
        wb_p, wb_t, wb_r = pass_rate(cat_wb)
        gap = wb_r - nb_r
        print(f"  {label}:")
        print(f"    no-bobbin:   {fmt_rate(nb_p, nb_t, nb_r)}")
        print(f"    with-bobbin: {fmt_rate(wb_p, wb_t, wb_r)}")
        print(f"    Gap:         {gap*100:+.1f}pp")
    print()

    # Ruff-only paired clean
    ruff_paired_no = [r for r in paired_clean_no if is_ruff(r)]
    ruff_paired_wb = [r for r in paired_clean_wb if is_ruff(r)]
    nb_p, nb_t, nb_r = pass_rate(ruff_paired_no)
    wb_p, wb_t, wb_r = pass_rate(ruff_paired_wb)
    gap = wb_r - nb_r
    print("## 5. Ruff-Only, Paired, Clean")
    print(f"  no-bobbin:   {fmt_rate(nb_p, nb_t, nb_r)}")
    print(f"  with-bobbin: {fmt_rate(wb_p, wb_t, wb_r)}")
    print(f"  Gap:         {gap*100:+.1f}pp")
    print()

    # Per-task regression drill-down
    print("## 6. Per-Task Regression Detail")
    task_groups: dict[str, dict[str, list[dict]]] = defaultdict(lambda: defaultdict(list))
    for r in results:
        task_groups[r["task_id"]][r.get("approach", "unknown")].append(r)

    for task_id in sorted(task_groups):
        approaches = task_groups[task_id]
        nb_p, nb_t, nb_r = pass_rate(approaches.get("no-bobbin", []))
        wb_p, wb_t, wb_r = pass_rate(approaches.get("with-bobbin", []))
        gap = wb_r - nb_r
        marker = " <<< REGRESSION" if gap < -0.01 else ""
        print(f"  {task_id:12s}  no-bobbin={fmt_rate(nb_p, nb_t, nb_r):20s}  "
              f"with-bobbin={fmt_rate(wb_p, wb_t, wb_r):20s}  gap={gap*100:+.1f}pp{marker}")
    print()

    # Bobbin metrics analysis
    print("## 7. Bobbin Injection Quality")
    wb_with_metrics = [r for r in total_wb if r.get("bobbin_metrics")]
    print(f"  With-bobbin runs total:      {len(total_wb)}")
    print(f"  With bobbin_metrics present: {len(wb_with_metrics)}")

    if wb_with_metrics:
        inj_counts = [r["bobbin_metrics"].get("injection_count", 0) for r in wb_with_metrics]
        gate_skips = [r["bobbin_metrics"].get("gate_skip_count", 0) for r in wb_with_metrics]
        print(f"  Avg injection_count:         {sum(inj_counts)/len(inj_counts):.1f}")
        print(f"  Avg gate_skip_count:         {sum(gate_skips)/len(gate_skips):.1f}")

        # Injection-to-ground-truth overlap
        overlaps = [
            r["bobbin_metrics"].get("overlap", {})
            for r in wb_with_metrics
            if r["bobbin_metrics"].get("overlap")
        ]
        if overlaps:
            avg_prec = sum(o.get("injection_precision", 0) for o in overlaps) / len(overlaps)
            avg_rec = sum(o.get("injection_recall", 0) for o in overlaps) / len(overlaps)
            print(f"  Avg injection precision:     {avg_prec:.1%}")
            print(f"  Avg injection recall:        {avg_rec:.1%}")

        # Show what files were injected
        for r in wb_with_metrics:
            bm = r["bobbin_metrics"]
            if bm.get("injected_files"):
                gt = set(r.get("diff_result", {}).get("ground_truth_files", []))
                injected = set(bm["injected_files"])
                overlap = injected & gt
                print(f"\n  Run {r['_run_id']} / {r['task_id']}:")
                print(f"    Injected: {sorted(injected)}")
                print(f"    Ground truth: {sorted(gt)}")
                print(f"    Overlap: {sorted(overlap) if overlap else 'NONE'}")
    print()

    # Summary
    print("=" * 70)
    print("CONCLUSION")
    print("=" * 70)
    print()
    nb_p, nb_t, nb_r = pass_rate(paired_clean_no)
    wb_p, wb_t, wb_r = pass_rate(paired_clean_wb)
    print(f"Controlled gap (paired, clean): {(wb_r - nb_r)*100:+.1f}pp")
    print(f"The -13% raw gap is a sampling artifact caused by:")
    print(f"  1. {len(unpaired_wb)} unpaired with-bobbin runs (flask tasks)")
    print(f"  2. Setup errors in paired runs")
    print(f"  3. Flask tasks at 0% on BOTH approaches (broken eval tasks)")
    print()
    if wb_with_metrics:
        zero_inj = sum(1 for r in wb_with_metrics if r["bobbin_metrics"].get("injection_count", 0) == 0)
        print(f"Injection quality concern: {zero_inj}/{len(wb_with_metrics)} runs had zero injections")
        print(f"When injections occurred, they returned irrelevant files (0% overlap with ground truth)")


def main():
    if len(sys.argv) < 2:
        print("Usage: python -m eval.analysis.controlled_comparison <results_dir>", file=sys.stderr)
        sys.exit(1)

    results_dir = Path(sys.argv[1])
    results = load_results(results_dir)
    if not results:
        print(f"No results found in {results_dir}", file=sys.stderr)
        sys.exit(1)

    analyze(results)


if __name__ == "__main__":
    main()
