#!/usr/bin/env python3
"""Recount the context-injection study directly from its per-run artifacts.

``scripts/paper_stats.py`` works from means transcribed out of the paper, on
purpose: it shows what the *reported* numbers do and do not support.  This
script is the other half, and answers a different question --- what is
actually in ``eval/results/runs/``.

It exists because the paper's run counts were not mutually consistent (85 /
96 / 108 for one study) and because Tables 5 and 6 were reported without
dispersion and without a stated inclusion rule.  Every table below is
regenerated from ``eval/results/runs/<run-id>/<task>_<approach>_<n>.json``,
which is the authoritative record.

Two inclusion rules matter and are reported separately rather than blended:

* ``status`` --- a run that terminated in ``bobbin_setup_error`` produced no
  measurement.  It is not an F1 of 0.0, and counting it as one manufactures a
  regression out of a harness failure.
* Flask --- the five Flask tasks are withdrawn (paper section 4.2).  Any
  aggregate over them is an aggregate over fixtures with a 0% pass rate on
  both arms.

Usage:
    python3 scripts/paper_census.py [--runs eval/results/runs]

The aggregate that the paper could not test --- Table 5 was published without
dispersion --- is tested here, because the per-run artifacts carry everything
the test needs.  scipy is used for that section only; without it the
descriptive tables still print.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
from collections import Counter, defaultdict
from pathlib import Path

try:
    from scipy import stats
except ImportError:  # pragma: no cover - the descriptive tables do not need it
    stats = None

WITHDRAWN_PREFIXES = ("flask-",)

# run-directory -> manifest.json, populated by load()
MANIFESTS: dict[str, dict] = {}


def load(runs_dir: Path) -> list[dict]:
    """Every per-run result JSON under *runs_dir*, manifests excluded."""
    out = []
    for path in sorted(runs_dir.glob("*/*.json")):
        if path.name == "manifest.json":
            continue
        record = json.loads(path.read_text())
        record["_path"] = str(path)
        manifest = path.parent / "manifest.json"
        if manifest.exists() and str(path.parent) not in MANIFESTS:
            MANIFESTS[str(path.parent)] = json.loads(manifest.read_text())
        out.append(record)
    if not out:
        raise SystemExit(f"no run artifacts under {runs_dir}")
    return out


def serving_model(record: dict) -> str:
    """The model that actually served a run, as its own artifact records it.

    ``model_usage`` is authoritative where it exists.  Haiku appears in it as a
    secondary model --- Claude Code's own internal use --- and is never the
    agent under test, so it is skipped when a primary model is present.  Older
    artifacts predate usage recording entirely and can only offer the model the
    manifest *declared*, which is a plan rather than an observation and is
    labelled as such.
    """
    usage = (record.get("agent_result") or {}).get("model_usage") or {}
    primary = [m for m in usage if "haiku" not in m]
    if primary:
        return f"{primary[0]} (confirmed)"
    manifest = MANIFESTS.get(str(Path(record["_path"]).parent), {})
    if manifest.get("model"):
        return f"{manifest['model']} (declared only)"
    return "no model record"


def withdrawn(record: dict) -> bool:
    return record["task_id"].startswith(WITHDRAWN_PREFIXES)


def is_ablation(record: dict) -> bool:
    return "+" in record["approach"]


def arm(record: dict) -> str:
    """The ablation flag, or the bare approach for the two primary arms."""
    approach = record["approach"]
    return approach.split("+", 1)[1] if "+" in approach else approach


def summarise(records: list[dict]) -> dict:
    """Mean and sample SD of every reported metric over *records*."""

    def stat(values):
        if not values:
            return (None, None)
        sd = statistics.stdev(values) if len(values) > 1 else 0.0
        return (statistics.mean(values), sd)

    return {
        "n": len(records),
        "f1": stat([r["diff_result"]["f1"] for r in records]),
        "precision": stat([r["diff_result"]["file_precision"] for r in records]),
        "recall": stat([r["diff_result"]["file_recall"] for r in records]),
        "pass_rate": stat([float(r["test_result"]["passed"]) for r in records]),
        "duration": stat([r["agent_result"]["duration_seconds"] for r in records]),
    }


def fmt(pair: tuple, places: int = 3, pct: bool = False) -> str:
    mean, sd = pair
    if mean is None:
        return "--"
    if pct:
        return f"{mean * 100:.1f}% +/- {sd * 100:.1f}"
    return f"{mean:.{places}f} +/- {sd:.{places}f}"


def section(title: str) -> None:
    print(f"\n{title}\n{'=' * len(title)}")


def report_counts(records: list[dict]) -> None:
    section("1. Run census --- what is in the artifact store")

    statuses = Counter(r["status"] for r in records)
    complete = [r for r in records if r["status"] == "completed"]
    ablation = [r for r in complete if is_ablation(r)]
    primary = [r for r in complete if not is_ablation(r)]

    print(f"result files            {len(records):>4}")
    for status, count in sorted(statuses.items()):
        print(f"  {status:<22}{count:>4}")
    print(f"run directories         {len({Path(r['_path']).parent for r in records}):>4}")
    print()
    print(f"completed runs          {len(complete):>4}")
    print(f"  ablation arms         {len(ablation):>4}   (all on ruff-001)")
    print(f"  primary arms          {len(primary):>4}")
    print(f"    withdrawn (Flask)   {len([r for r in primary if withdrawn(r)]):>4}")
    print(f"    reported tasks      {len([r for r in primary if not withdrawn(r)]):>4}")
    print()
    print("Runs that produced no measurement, by task and arm:")
    failed = Counter(
        (r["task_id"], r["approach"]) for r in records if r["status"] != "completed"
    )
    for (task, approach), count in sorted(failed.items()):
        print(f"  {task:<12}{approach:<34}{count:>3}")


def report_aggregate(records: list[dict]) -> None:
    section("2. Aggregate over the primary arms (paper Table 5)")

    complete = [r for r in records if r["status"] == "completed" and not is_ablation(r)]
    scopes = [
        ("all tasks, Flask included", complete),
        ("reported tasks only", [r for r in complete if not withdrawn(r)]),
        ("withdrawn Flask tasks only", [r for r in complete if withdrawn(r)]),
    ]

    for label, scope in scopes:
        print(f"\n{label} ({len(scope)} runs)")
        header = f"  {'arm':<14}{'N':>4}  {'F1':>17}{'precision':>17}{'recall':>17}{'pass rate':>19}{'duration (s)':>19}"
        print(header)
        for approach in ("no-bobbin", "with-bobbin"):
            group = [r for r in scope if r["approach"] == approach]
            if not group:
                continue
            s = summarise(group)
            print(
                f"  {approach:<14}{s['n']:>4}  {fmt(s['f1']):>17}{fmt(s['precision']):>17}"
                f"{fmt(s['recall']):>17}{fmt(s['pass_rate'], pct=True):>19}"
                f"{fmt(s['duration'], places=1):>19}"
            )


def report_per_task(records: list[dict]) -> None:
    section("3. Per task (paper Table 6)")

    complete = [r for r in records if r["status"] == "completed" and not is_ablation(r)]
    failed = Counter((r["task_id"], r["approach"]) for r in records if r["status"] != "completed")
    tasks = sorted({r["task_id"] for r in records if not is_ablation(r)})

    print(f"  {'task':<12}{'no-bobbin':>26}{'with-bobbin':>26}   note")
    for task in tasks:
        cells = []
        for approach in ("no-bobbin", "with-bobbin"):
            group = [r for r in complete if r["task_id"] == task and r["approach"] == approach]
            cells.append(f"N={len(group)} {fmt(summarise(group)['f1'])}" if group else "no completed runs")
        errors = sum(c for (t, _), c in failed.items() if t == task)
        note = []
        if withdrawn({"task_id": task}):
            note.append("withdrawn")
        if errors:
            note.append(f"{errors} run(s) failed to measure")
        print(f"  {task:<12}{cells[0]:>26}{cells[1]:>26}   {', '.join(note)}")


def report_ablation(records: list[dict]) -> None:
    section("4. Ablation arms on ruff-001 (paper Table 7)")

    by_arm = defaultdict(list)
    for r in records:
        if r["status"] == "completed" and r["task_id"] == "ruff-001":
            by_arm[arm(r)].append(r)

    order = ["no-bobbin", "with-bobbin"] + sorted(k for k in by_arm if k not in ("no-bobbin", "with-bobbin"))
    print(f"  {'arm':<24}{'N':>4}  {'F1':>17}{'pass rate':>19}")
    for key in order:
        s = summarise(by_arm[key])
        print(f"  {key:<24}{s['n']:>4}  {fmt(s['f1']):>17}{fmt(s['pass_rate'], pct=True):>19}")


def report_models(records: list[dict]) -> None:
    section("6. Which model actually served each run")

    complete = [r for r in records if r["status"] == "completed"]
    counts = Counter(serving_model(r) for r in complete)
    print(f"  {'serving model':<52}{'runs':>6}")
    for model, count in sorted(counts.items(), key=lambda kv: -kv[1]):
        print(f"  {model:<52}{count:>6}")
    unattributed = sum(c for m, c in counts.items() if "confirmed" not in m)
    print(f"\n  {unattributed} of {len(complete)} completed runs cannot be attributed")
    print("  to a serving model from their own artifact.")

    print("\n  ruff-001 arms by serving model --- the ablation comparison:")
    for approach in sorted({r["approach"] for r in complete if r["task_id"] == "ruff-001"}):
        group = [r for r in complete if r["task_id"] == "ruff-001" and r["approach"] == approach]
        by_model = Counter(serving_model(r) for r in group)
        rendered = ", ".join(f"{m}: {c}" for m, c in sorted(by_model.items()))
        print(f"    {arm({'approach': approach}):<24}N={len(group)}  {rendered}")
    print()
    print("  Every ablation arm is pure Sonnet 4.5.  The with-bobbin baseline")
    print("  they are all compared against is not --- see paper section 5.1.1.")


def report_aggregate_test(records: list[dict]) -> None:
    section("5. Is the aggregate distinguishable from noise?")

    if stats is None:
        print("  scipy not installed; skipping (pip install scipy)")
        return

    complete = [r for r in records if r["status"] == "completed" and not is_ablation(r)]
    scopes = [
        ("reported tasks only", [r for r in complete if not withdrawn(r)]),
        ("all tasks, Flask included", complete),
        ("withdrawn Flask tasks only", [r for r in complete if withdrawn(r)]),
    ]

    print(f"  {'scope':<28}{'delta F1':>10}{'t':>8}{'df':>7}{'p':>8}   95% CI")
    for label, scope in scopes:
        treat = [r["diff_result"]["f1"] for r in scope if r["approach"] == "with-bobbin"]
        control = [r["diff_result"]["f1"] for r in scope if r["approach"] == "no-bobbin"]
        if len(treat) < 2 or len(control) < 2:
            continue
        m1, s1, n1 = statistics.mean(treat), statistics.stdev(treat), len(treat)
        m2, s2, n2 = statistics.mean(control), statistics.stdev(control), len(control)
        se = math.sqrt(s1**2 / n1 + s2**2 / n2)
        t = (m1 - m2) / se
        df = (s1**2 / n1 + s2**2 / n2) ** 2 / (
            (s1**2 / n1) ** 2 / (n1 - 1) + (s2**2 / n2) ** 2 / (n2 - 1)
        )
        p = 2 * (1 - stats.t.cdf(abs(t), df))
        crit = stats.t.ppf(0.975, df)
        delta = m1 - m2
        print(
            f"  {label:<28}{delta:>+10.3f}{t:>8.2f}{df:>7.1f}{p:>8.3f}"
            f"   [{delta - crit * se:+.3f}, {delta + crit * se:+.3f}]"
        )

    print()
    print("  Welch's t-test, two-sided.  Runs are pooled across tasks, so task")
    print("  difficulty is a between-arm confound wherever the arms are not")
    print("  balanced per task --- see the per-task counts in section 3.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--runs", type=Path, default=Path("eval/results/runs"))
    args = parser.parse_args()

    records = load(args.runs)
    report_counts(records)
    report_aggregate(records)
    report_per_task(records)
    report_ablation(records)
    report_aggregate_test(records)
    report_models(records)
    print()


if __name__ == "__main__":
    main()
