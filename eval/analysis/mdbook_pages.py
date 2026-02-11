"""Generate mdbook-compatible markdown pages from eval results.

Reads result JSON files and task YAML definitions, then produces markdown
pages with matplotlib SVG charts for the bobbin documentation site.
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

import yaml

from analysis.mpl_charts import (
    apply_dracula_theme,
    box_plot_chart,
    duration_chart,
    grouped_bar_chart,
    heatmap_chart,
    multi_metric_chart,
    trend_chart,
)

logger = logging.getLogger(__name__)


class PageGenError(Exception):
    """Raised when page generation fails."""


def _load_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load all completed result JSON files.

    Scans ``results/runs/*/*.json`` (run-based layout) first, then falls
    back to ``results/*.json`` (legacy flat layout).
    """
    results: list[dict[str, Any]] = []

    # Run-based layout.
    runs_dir = results_dir / "runs"
    if runs_dir.is_dir():
        for f in sorted(runs_dir.glob("*/*.json")):
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and data.get("status") == "completed":
                    results.append(data)
            except (json.JSONDecodeError, OSError):
                pass

    # Legacy flat layout.
    for f in sorted(results_dir.glob("*.json")):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            if isinstance(data, dict) and data.get("status") == "completed":
                results.append(data)
        except (json.JSONDecodeError, OSError):
            pass

    return results


def _load_all_runs(results_dir: Path) -> dict[str, list[dict[str, Any]]]:
    """Load results grouped by run_id.

    Returns ``{run_id: [results]}`` with ``"legacy"`` as the key for
    results that lack a run_id.
    """
    by_run: dict[str, list[dict[str, Any]]] = {}
    for r in _load_results(results_dir):
        run_id = r.get("run_id", "legacy")
        by_run.setdefault(run_id, []).append(r)
    return dict(sorted(by_run.items()))


def _get_latest_run(results_dir: Path) -> tuple[str, list[dict[str, Any]]]:
    """Return the most recent (run_id, results) pair.

    Run IDs sort chronologically by design (YYYYMMDD-HHMMSS-XXXX).
    """
    all_runs = _load_all_runs(results_dir)
    if not all_runs:
        return ("", [])
    run_id = max(all_runs.keys())
    return (run_id, all_runs[run_id])


def _load_judge_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load judge results if available.

    Checks run directories first, then falls back to legacy flat layout.
    """
    all_judgements: list[dict[str, Any]] = []

    # Run-based layout.
    runs_dir = results_dir / "runs"
    if runs_dir.is_dir():
        for f in sorted(runs_dir.glob("*/judge_results.json")):
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, list):
                    all_judgements.extend(data)
            except (json.JSONDecodeError, OSError):
                pass

    # Legacy flat layout.
    judge_file = results_dir / "judge_results.json"
    if judge_file.exists():
        try:
            data = json.loads(judge_file.read_text(encoding="utf-8"))
            if isinstance(data, list):
                all_judgements.extend(data)
        except (json.JSONDecodeError, OSError):
            pass

    return all_judgements


def _load_tasks(tasks_dir: Path) -> dict[str, dict[str, Any]]:
    """Load all task YAML files, keyed by task id."""
    tasks = {}
    for f in sorted(tasks_dir.glob("*.yaml")):
        try:
            task = yaml.safe_load(f.read_text(encoding="utf-8"))
            if isinstance(task, dict) and "id" in task:
                tasks[task["id"]] = task
        except (yaml.YAMLError, OSError):
            pass
    return tasks


def _group_by_task(results: list[dict]) -> dict[str, list[dict]]:
    """Group results by task_id."""
    groups: dict[str, list[dict]] = {}
    for r in results:
        tid = r.get("task_id", "unknown")
        groups.setdefault(tid, []).append(r)
    return dict(sorted(groups.items()))


def _group_by_approach(results: list[dict]) -> dict[str, list[dict]]:
    """Group results by approach."""
    groups: dict[str, list[dict]] = {}
    for r in results:
        approach = r.get("approach", "unknown")
        groups.setdefault(approach, []).append(r)
    return dict(sorted(groups.items()))


def _safe_avg(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def _compute_approach_stats(results: list[dict]) -> dict[str, Any]:
    """Compute aggregate stats for one approach."""
    test_passes = [r for r in results if r.get("test_result", {}).get("passed")]
    pass_rate = len(test_passes) / len(results) if results else 0.0

    precisions = [r["diff_result"]["file_precision"] for r in results if r.get("diff_result", {}).get("file_precision") is not None]
    recalls = [r["diff_result"]["file_recall"] for r in results if r.get("diff_result", {}).get("file_recall") is not None]
    f1s = [r["diff_result"]["f1"] for r in results if r.get("diff_result", {}).get("f1") is not None]
    durations = [r["agent_result"]["duration_seconds"] for r in results if r.get("agent_result", {}).get("duration_seconds") is not None]

    costs = [r["agent_result"]["cost_usd"] for r in results if r.get("agent_result", {}).get("cost_usd") is not None]
    input_tokens = [r["agent_result"]["input_tokens"] for r in results if r.get("agent_result", {}).get("input_tokens") is not None]
    output_tokens = [r["agent_result"]["output_tokens"] for r in results if r.get("agent_result", {}).get("output_tokens") is not None]

    return {
        "count": len(results),
        "test_pass_rate": pass_rate,
        "avg_file_precision": _safe_avg(precisions),
        "avg_file_recall": _safe_avg(recalls),
        "avg_f1": _safe_avg(f1s),
        "avg_duration_seconds": _safe_avg(durations),
        "avg_cost_usd": _safe_avg(costs),
        "avg_input_tokens": _safe_avg(input_tokens),
        "avg_output_tokens": _safe_avg(output_tokens),
        "has_cost_data": len(costs) > 0,
        "f1_values": f1s,
        "duration_values": durations,
    }


def _pick_best(runs: list[dict]) -> dict | None:
    """Pick best attempt: prefer passing, then highest F1."""
    if not runs:
        return None
    passing = [r for r in runs if r.get("test_result", {}).get("passed")]
    pool = passing if passing else runs
    return max(pool, key=lambda r: r.get("diff_result", {}).get("f1", 0.0))


def _format_pct(val: float) -> str:
    return f"{val * 100:.1f}%"


def _format_duration(seconds: float) -> str:
    if seconds < 60:
        return f"{seconds:.0f}s"
    return f"{seconds / 60:.1f}m"


def _save_chart(svg: str, charts_dir: Path, filename: str) -> str:
    """Save an SVG chart and return the markdown image reference."""
    if not svg:
        return ""
    charts_dir.mkdir(parents=True, exist_ok=True)
    (charts_dir / filename).write_text(svg, encoding="utf-8")
    return f"![{filename}](./charts/{filename})"


# -- Page generators --


def generate_summary_page(
    results: list[dict],
    judge_results: list[dict],
    tasks: dict[str, dict],
    *,
    charts_dir: Path | None = None,
    all_runs: dict[str, list[dict]] | None = None,
) -> str:
    """Generate the results summary page."""
    by_approach = _group_by_approach(results)
    stats = {a: _compute_approach_stats(runs) for a, runs in by_approach.items()}
    by_task = _group_by_task(results)

    lines = [
        "# Results Summary",
        "",
    ]

    # Summary comparison table.
    approaches = sorted(stats.keys())
    has_both = len(approaches) == 2

    lines.append("## Overall Comparison")
    lines.append("")
    header = "| Metric |"
    sep = "|--------|"
    for a in approaches:
        header += f" {a} |"
        sep += ":---:|"
    if has_both:
        header += " Delta |"
        sep += ":---:|"
    lines.append(header)
    lines.append(sep)

    # Check if any approach has cost data.
    has_cost = any(s.get("has_cost_data") for s in stats.values())

    metrics = [
        ("Runs", "count", False),
        ("Test Pass Rate", "test_pass_rate", True),
        ("Avg Precision", "avg_file_precision", True),
        ("Avg Recall", "avg_file_recall", True),
        ("Avg F1", "avg_f1", True),
        ("Avg Duration", "avg_duration_seconds", False),
    ]

    if has_cost:
        metrics.extend([
            ("Avg Cost", "avg_cost_usd", False),
            ("Avg Input Tokens", "avg_input_tokens", False),
            ("Avg Output Tokens", "avg_output_tokens", False),
        ])

    for label, key, is_pct in metrics:
        row = f"| {label} |"
        vals = []
        for a in approaches:
            v = stats[a].get(key, 0)
            vals.append(v)
            if is_pct:
                row += f" {_format_pct(v)} |"
            elif key == "count":
                row += f" {int(v)} |"
            elif key == "avg_duration_seconds":
                row += f" {_format_duration(v)} |"
            elif key == "avg_cost_usd":
                row += f" ${v:.2f} |"
            elif key in ("avg_input_tokens", "avg_output_tokens"):
                row += f" {int(v):,} |"
            else:
                row += f" {v:.1f} |"
        if has_both:
            if is_pct:
                diff = vals[1] - vals[0]
                if abs(diff) < 0.001:
                    row += " — |"
                else:
                    sign = "+" if diff > 0 else ""
                    row += f" {sign}{diff * 100:.1f}pp |"
            elif key in ("avg_duration_seconds", "avg_cost_usd") and vals[0] > 0:
                pct = (vals[1] - vals[0]) / vals[0] * 100
                row += f" {pct:+.0f}% |"
            else:
                row += " |"
        lines.append(row)

    lines.append("")

    # Multi-metric chart: precision/recall/F1 overview.
    if charts_dir and stats:
        apply_dracula_theme()
        svg = multi_metric_chart(stats, title="Precision / Recall / F1")
        ref = _save_chart(svg, charts_dir, "summary_metrics.svg")
        if ref:
            lines.append("## Metric Overview")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Grouped bar chart: F1 scores by task.
    chart_groups = []
    for task_id, task_results in by_task.items():
        task_by_approach = _group_by_approach(task_results)
        values = {}
        for a in approaches:
            a_stats = _compute_approach_stats(task_by_approach.get(a, []))
            values[a] = a_stats["avg_f1"]
        short_id = task_id.split("-")[0] + "-" + task_id.split("-")[1] if "-" in task_id else task_id
        chart_groups.append({"label": short_id, "values": values})

    if chart_groups:
        lines.append("## F1 Score by Task")
        lines.append("")
        if charts_dir:
            apply_dracula_theme()
            svg = grouped_bar_chart(chart_groups, title="F1 Score Comparison")
            ref = _save_chart(svg, charts_dir, "summary_f1_by_task.svg")
            if ref:
                lines.append('<div class="eval-chart">')
                lines.append("")
                lines.append(ref)
                lines.append("")
                lines.append("</div>")
                lines.append("")
        else:
            # Inline fallback (no charts dir).
            apply_dracula_theme()
            svg = grouped_bar_chart(chart_groups, title="F1 Score Comparison")
            if svg:
                lines.append('<div class="eval-chart">')
                lines.append("")
                lines.append(svg)
                lines.append("")
                lines.append("</div>")
                lines.append("")

    # Box plot: F1 distribution when multiple attempts exist.
    f1_by_approach: dict[str, list[float]] = {}
    for a in approaches:
        f1_by_approach[a] = stats[a].get("f1_values", [])
    has_multiple = any(len(v) > 1 for v in f1_by_approach.values())
    if charts_dir and has_multiple:
        apply_dracula_theme()
        svg = box_plot_chart(f1_by_approach, metric_name="F1", title="F1 Score Distribution")
        ref = _save_chart(svg, charts_dir, "summary_f1_boxplot.svg")
        if ref:
            lines.append("## Score Distribution")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Duration chart.
    dur_by_approach: dict[str, list[float]] = {}
    for a in approaches:
        dur_by_approach[a] = stats[a].get("duration_values", [])
    has_durations = any(dur_by_approach.values())
    if charts_dir and has_durations:
        apply_dracula_theme()
        svg = duration_chart(dur_by_approach, title="Duration Comparison")
        ref = _save_chart(svg, charts_dir, "summary_duration.svg")
        if ref:
            lines.append("## Duration")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Quick trend: last 5 runs (if historical data available).
    if charts_dir and all_runs and len(all_runs) > 1:
        recent_ids = sorted(all_runs.keys())[-5:]
        runs_data = []
        for rid in recent_ids:
            run_results = all_runs[rid]
            run_by_approach = _group_by_approach(run_results)
            values = {}
            for a, a_results in run_by_approach.items():
                a_stats = _compute_approach_stats(a_results)
                values[a] = a_stats["avg_f1"]
            date = rid[:8] if len(rid) >= 8 else rid
            runs_data.append({"run_id": rid, "date": date, "values": values})

        apply_dracula_theme()
        svg = trend_chart(runs_data, metric="avg_f1", title="F1 Trend (Recent)")
        ref = _save_chart(svg, charts_dir, "summary_trend.svg")
        if ref:
            lines.append("## Recent Trend")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")
            lines.append("[Full historical trends](./trends.md)")
            lines.append("")

    # Per-task mini-table.
    lines.append("## Per-Task Results")
    lines.append("")
    if has_cost:
        lines.append("| Task | Language | Difficulty | Approach | Tests | Precision | Recall | F1 | Duration | Cost |")
        lines.append("|------|----------|:----------:|----------|:-----:|:---------:|:------:|:--:|:--------:|-----:|")
    else:
        lines.append("| Task | Language | Difficulty | Approach | Tests | Precision | Recall | F1 | Duration |")
        lines.append("|------|----------|:----------:|----------|:-----:|:---------:|:------:|:--:|:--------:|")

    for task_id, task_results in by_task.items():
        task_def = tasks.get(task_id, {})
        lang = task_def.get("language", "—")
        diff = task_def.get("difficulty", "—")
        task_by_approach = _group_by_approach(task_results)

        for a in approaches:
            a_runs = task_by_approach.get(a, [])
            if not a_runs:
                continue
            s = _compute_approach_stats(a_runs)
            pass_str = _format_pct(s["test_pass_rate"])
            row = (
                f"| {task_id} | {lang} | {diff} | {a} "
                f"| {pass_str} | {_format_pct(s['avg_file_precision'])} "
                f"| {_format_pct(s['avg_file_recall'])} | {_format_pct(s['avg_f1'])} "
                f"| {_format_duration(s['avg_duration_seconds'])} |"
            )
            if has_cost:
                row = row.rstrip(" |") + f" ${s['avg_cost_usd']:.2f} |"
            lines.append(row)

    lines.append("")

    # Judge summary.
    if judge_results:
        lines.append("## LLM Judge Summary")
        lines.append("")
        by_pair: dict[str, list[dict]] = {}
        for j in judge_results:
            by_pair.setdefault(j.get("pair", ""), []).append(j)

        lines.append("| Comparison | Tasks | Winner Distribution |")
        lines.append("|------------|:-----:|---------------------|")
        for pair_label in sorted(by_pair):
            judgements = by_pair[pair_label]
            n = len(judgements)
            wins: dict[str, int] = {}
            for j in judgements:
                w = j.get("named_winner", "?")
                wins[w] = wins.get(w, 0) + 1
            parts = [f"{name}: {count}" for name, count in sorted(wins.items())]
            a_label = judgements[0].get("a_label", "A")
            b_label = judgements[0].get("b_label", "B")
            lines.append(f"| {a_label} vs {b_label} | {n} | {', '.join(parts)} |")

        lines.append("")

    # Strip trailing blank lines to avoid MD012 (no-multiple-blanks).
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines) + "\n"


def generate_project_page(
    results: list[dict],
    tasks: dict[str, dict],
) -> str:
    """Generate the project catalog page with LOC and bobbin stats."""
    lines = [
        "# Project Catalog",
        "",
        "Projects used in bobbin evaluations, with codebase statistics.",
        "",
    ]

    # Group by repo.
    by_repo: dict[str, list[dict]] = {}
    for r in results:
        repo = r.get("task", {}).get("repo", "unknown")
        by_repo.setdefault(repo, []).append(r)

    for repo, repo_results in sorted(by_repo.items()):
        lines.append(f"## {repo}")
        lines.append("")

        # Find a result with project_metadata.
        meta = None
        bobbin_meta = None
        for r in repo_results:
            if r.get("project_metadata") and not meta:
                meta = r["project_metadata"]
            if r.get("bobbin_metadata") and not bobbin_meta:
                bobbin_meta = r["bobbin_metadata"]

        if meta:
            # LOC breakdown table.
            # Use tokei's "Total" entry if present, otherwise sum individual languages.
            tokei_total = meta.get("Total", {}) if isinstance(meta.get("Total"), dict) else {}
            lang_entries = {k: v for k, v in meta.items() if isinstance(v, dict) and k != "Total"}
            total_code = tokei_total.get("code", sum(v.get("code", 0) for v in lang_entries.values()))
            total_lines = tokei_total.get("lines", sum(v.get("lines", 0) for v in lang_entries.values()))

            lines.append("### Lines of Code")
            lines.append("")
            lines.append("| Language | Files | Code | Comments | Blanks | Total |")
            lines.append("|----------|------:|-----:|---------:|-------:|------:|")

            sorted_langs = sorted(
                [(k, v) for k, v in meta.items()
                 if isinstance(v, dict) and v.get("code", 0) > 0 and k != "Total"],
                key=lambda x: x[1].get("code", 0),
                reverse=True,
            )
            total_files = sum(v.get("files", 0) for _, v in sorted_langs)
            for lang, lang_stats in sorted_langs[:10]:
                lines.append(
                    f"| {lang} | {lang_stats.get('files', 0):,} | {lang_stats.get('code', 0):,} "
                    f"| {lang_stats.get('comments', 0):,} | {lang_stats.get('blanks', 0):,} "
                    f"| {lang_stats.get('lines', 0):,} |"
                )
            if total_code:
                lines.append(f"| **Total** | **{total_files:,}** | **{total_code:,}** | | | **{total_lines:,}** |")
            lines.append("")

        if bobbin_meta:
            lines.append("### Bobbin Index Stats")
            lines.append("")
            lines.append(f"- **Index duration**: {bobbin_meta.get('index_duration_seconds', '?')}s")
            if bobbin_meta.get("total_files"):
                lines.append(f"- **Files indexed**: {bobbin_meta['total_files']:,}")
            if bobbin_meta.get("total_chunks"):
                lines.append(f"- **Chunks**: {bobbin_meta['total_chunks']:,}")
            if bobbin_meta.get("total_embeddings"):
                lines.append(f"- **Embeddings**: {bobbin_meta['total_embeddings']:,}")
            if bobbin_meta.get("languages"):
                lines.append(f"- **Languages detected**: {', '.join(bobbin_meta['languages'])}")
            lines.append("")

        if not meta and not bobbin_meta:
            lines.append("*No metadata available. Re-run evals to collect project stats.*")
            lines.append("")

    # Strip trailing blank lines to avoid MD012 (no-multiple-blanks).
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines) + "\n"


def generate_task_detail_page(
    project_name: str,
    language: str,
    task_ids: list[str],
    results: list[dict],
    tasks: dict[str, dict],
    judge_results: list[dict],
    *,
    charts_dir: Path | None = None,
) -> str:
    """Generate a per-project task detail page (e.g. flask.md, ruff.md)."""
    lines = [
        f"# {project_name} ({language})",
        "",
    ]

    by_task = _group_by_task(results)

    for task_id in task_ids:
        task_results = by_task.get(task_id, [])
        task_def = tasks.get(task_id, {})

        if not task_results:
            continue

        difficulty = task_def.get("difficulty", "medium")
        badge_class = {"easy": "eval-easy", "medium": "eval-medium", "hard": "eval-hard"}.get(difficulty, "eval-medium")
        lines.append(f'## {task_id} <span class="{badge_class}">{difficulty}</span>')
        lines.append("")

        # Commit info.
        repo = task_def.get("repo", "")
        commit = task_def.get("commit", "")
        if repo and commit:
            short_hash = commit[:10]
            lines.append(f"**Commit**: [{short_hash}](https://github.com/{repo}/commit/{commit})")
        lines.append("")

        # Prompt (collapsible).
        desc = task_def.get("description", "").strip()
        if desc:
            # Escape underscores to prevent markdown emphasis interpretation.
            desc_escaped = desc.replace("_", r"\_")
            lines.append("<details>")
            lines.append("<summary>Task prompt</summary>")
            lines.append("")
            lines.append(f"> {desc_escaped}")
            lines.append("")
            lines.append("</details>")
            lines.append("")

        # Results table.
        task_by_approach = _group_by_approach(task_results)
        approaches = sorted(task_by_approach.keys())

        # Check if any result in this task has cost data.
        task_has_cost = any(
            r.get("agent_result", {}).get("cost_usd") is not None
            for r in task_results
        )

        if task_has_cost:
            lines.append("| Approach | Tests Pass | Precision | Recall | F1 | Duration | Cost |")
            lines.append("|----------|:----------:|:---------:|:------:|:--:|:--------:|-----:|")
        else:
            lines.append("| Approach | Tests Pass | Precision | Recall | F1 | Duration |")
            lines.append("|----------|:----------:|:---------:|:------:|:--:|:--------:|")

        for a in approaches:
            s = _compute_approach_stats(task_by_approach[a])
            row = (
                f"| {a} | {_format_pct(s['test_pass_rate'])} "
                f"| {_format_pct(s['avg_file_precision'])} "
                f"| {_format_pct(s['avg_file_recall'])} "
                f"| {_format_pct(s['avg_f1'])} "
                f"| {_format_duration(s['avg_duration_seconds'])} |"
            )
            if task_has_cost:
                row = row.rstrip(" |") + f" ${s['avg_cost_usd']:.2f} |"
            lines.append(row)

        lines.append("")

        # Box plot: score distributions (when multiple attempts).
        if charts_dir:
            safe_id = task_id.replace("/", "_")
            f1_data: dict[str, list[float]] = {}
            for a in approaches:
                a_stats = _compute_approach_stats(task_by_approach[a])
                f1_data[a] = a_stats.get("f1_values", [])
            has_multiple = any(len(v) > 1 for v in f1_data.values())
            if has_multiple:
                apply_dracula_theme()
                svg = box_plot_chart(f1_data, metric_name="F1", title=f"{task_id} F1 Distribution")
                ref = _save_chart(svg, charts_dir, f"{safe_id}_f1_boxplot.svg")
                if ref:
                    lines.append('<div class="eval-chart">')
                    lines.append("")
                    lines.append(ref)
                    lines.append("")
                    lines.append("</div>")
                    lines.append("")

            # Duration chart per task.
            dur_data: dict[str, list[float]] = {}
            for a in approaches:
                a_stats = _compute_approach_stats(task_by_approach[a])
                dur_data[a] = a_stats.get("duration_values", [])
            if any(dur_data.values()):
                apply_dracula_theme()
                svg = duration_chart(dur_data, title=f"{task_id} Duration")
                ref = _save_chart(svg, charts_dir, f"{safe_id}_duration.svg")
                if ref:
                    lines.append('<div class="eval-chart">')
                    lines.append("")
                    lines.append(ref)
                    lines.append("")
                    lines.append("</div>")
                    lines.append("")

        # Files touched vs ground truth.
        best_per_approach: dict[str, dict] = {}
        for a in approaches:
            best = _pick_best(task_by_approach.get(a, []))
            if best:
                best_per_approach[a] = best

        if best_per_approach:
            sample = next(iter(best_per_approach.values()))
            gt_files = sample.get("diff_result", {}).get("ground_truth_files", [])

            if gt_files:
                lines.append("**Ground truth files**: " + ", ".join(f"`{f}`" for f in gt_files))
                lines.append("")

            for a, best in best_per_approach.items():
                touched = best.get("diff_result", {}).get("files_touched", [])
                if touched:
                    lines.append(f"**Files touched ({a})**: " + ", ".join(f"`{f}`" for f in touched))

            lines.append("")

        # Judge results for this task.
        task_judge = [j for j in judge_results if j.get("task_id") == task_id]
        if task_judge:
            lines.append("**Judge verdict**:")
            lines.append("")
            for j in task_judge:
                winner = j.get("named_winner", "?")
                pair = f"{j.get('a_label', 'A')} vs {j.get('b_label', 'B')}"
                bias = " (bias detected)" if j.get("bias_detected") else ""
                lines.append(f"- {pair}: **{winner}**{bias}")
            lines.append("")

        lines.append("---")
        lines.append("")

    # Strip trailing blank lines to avoid MD012 (no-multiple-blanks).
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines) + "\n"


def generate_trends_page(
    all_runs: dict[str, list[dict]],
    *,
    charts_dir: Path | None = None,
) -> str:
    """Generate the historical trends page (trends.md)."""
    lines = [
        "# Historical Trends",
        "",
    ]

    if len(all_runs) < 2:
        lines.append("*Not enough runs for trend analysis. Run more evaluations to see trends.*")
        while lines and lines[-1] == "":
            lines.pop()
        return "\n".join(lines) + "\n"

    run_ids = sorted(all_runs.keys())

    # Build trend data.
    f1_runs_data = []
    pass_rate_runs_data = []
    duration_runs_data = []
    heatmap_runs_data = []

    for rid in run_ids:
        run_results = all_runs[rid]
        run_by_approach = _group_by_approach(run_results)
        date = rid[:8] if len(rid) >= 8 else rid

        f1_values: dict[str, float] = {}
        pass_values: dict[str, float] = {}
        dur_values: dict[str, float] = {}
        for a, a_results in run_by_approach.items():
            a_stats = _compute_approach_stats(a_results)
            f1_values[a] = a_stats["avg_f1"]
            pass_values[a] = a_stats["test_pass_rate"]
            dur_values[a] = a_stats["avg_duration_seconds"]

        f1_runs_data.append({"run_id": rid, "date": date, "values": f1_values})
        pass_rate_runs_data.append({"run_id": rid, "date": date, "values": pass_values})
        duration_runs_data.append({"run_id": rid, "date": date, "values": dur_values})

        # Heatmap data: per-task F1 for this run.
        run_by_task = _group_by_task(run_results)
        task_f1s = {}
        for tid, task_results in run_by_task.items():
            task_stats = _compute_approach_stats(task_results)
            task_f1s[tid] = task_stats["avg_f1"]
        heatmap_runs_data.append({"run_id": rid, "date": date, "tasks": task_f1s})

    # F1 trend.
    if charts_dir:
        apply_dracula_theme()
        svg = trend_chart(f1_runs_data, metric="avg_f1", title="F1 Score Over Time")
        ref = _save_chart(svg, charts_dir, "trend_f1.svg")
        if ref:
            lines.append("## F1 Trend")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Test pass rate trend.
    if charts_dir:
        apply_dracula_theme()
        svg = trend_chart(pass_rate_runs_data, metric="test_pass_rate", title="Test Pass Rate Over Time")
        ref = _save_chart(svg, charts_dir, "trend_pass_rate.svg")
        if ref:
            lines.append("## Test Pass Rate Trend")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Duration trend.
    if charts_dir:
        apply_dracula_theme()
        svg = trend_chart(duration_runs_data, metric="duration (s)", title="Duration Over Time")
        ref = _save_chart(svg, charts_dir, "trend_duration.svg")
        if ref:
            lines.append("## Duration Trend")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Per-run comparison table.
    lines.append("## Run Comparison")
    lines.append("")
    lines.append("| Run | Completed | Pass Rate | Avg F1 | Avg Duration |")
    lines.append("|-----|-----------|:---------:|:------:|:------------:|")
    for rid in run_ids:
        run_results = all_runs[rid]
        run_stats = _compute_approach_stats(run_results)
        date = rid[:8] if len(rid) >= 8 else rid
        lines.append(
            f"| {date} | {run_stats['count']} "
            f"| {_format_pct(run_stats['test_pass_rate'])} "
            f"| {_format_pct(run_stats['avg_f1'])} "
            f"| {_format_duration(run_stats['avg_duration_seconds'])} |"
        )
    lines.append("")

    # Task heatmap.
    if charts_dir and heatmap_runs_data:
        apply_dracula_theme()
        svg = heatmap_chart(heatmap_runs_data, metric="F1", title="F1 by Task x Run")
        ref = _save_chart(svg, charts_dir, "heatmap_f1.svg")
        if ref:
            lines.append("## Task Heatmap")
            lines.append("")
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(ref)
            lines.append("")
            lines.append("</div>")
            lines.append("")

    # Strip trailing blank lines to avoid MD012 (no-multiple-blanks).
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines) + "\n"


def generate_all_pages(
    results_dir: str,
    tasks_dir: str,
    output_dir: str,
    *,
    run_id: str | None = None,
    include_trends: bool = False,
) -> list[str]:
    """Generate all mdbook pages from eval results.

    Parameters
    ----------
    results_dir:
        Path to the results directory.
    tasks_dir:
        Path to the tasks directory.
    output_dir:
        Path to write generated markdown and charts.
    run_id:
        Specific run to publish. Default: latest.
    include_trends:
        If True, generate trends page from all historical runs.

    Returns a list of generated file paths (relative to output_dir).
    """
    rdir = Path(results_dir)
    tdir = Path(tasks_dir)
    odir = Path(output_dir)
    odir.mkdir(parents=True, exist_ok=True)
    charts_dir = odir / "charts"
    charts_dir.mkdir(parents=True, exist_ok=True)

    # Load all runs for trends; pick target run for main pages.
    all_runs = _load_all_runs(rdir)

    if run_id:
        results = all_runs.get(run_id, [])
        if not results:
            raise PageGenError(f"No completed results for run {run_id} in {rdir}")
    else:
        results = _load_results(rdir)

    if not results:
        raise PageGenError(f"No completed results in {rdir}")

    tasks = _load_tasks(tdir)
    judge_results = _load_judge_results(rdir)

    generated = []

    # Summary page.
    summary = generate_summary_page(
        results,
        judge_results,
        tasks,
        charts_dir=charts_dir,
        all_runs=all_runs if include_trends or len(all_runs) > 1 else None,
    )
    (odir / "summary.md").write_text(summary, encoding="utf-8")
    generated.append("summary.md")

    # Projects page.
    projects = generate_project_page(results, tasks)
    (odir / "projects.md").write_text(projects, encoding="utf-8")
    generated.append("projects.md")

    # Per-project detail pages.
    by_project: dict[str, list[str]] = {}
    for task_id, task in tasks.items():
        repo = task.get("repo", "")
        project_name = repo.split("/")[-1] if repo else "unknown"
        by_project.setdefault(project_name, []).append(task_id)

    for project_name, task_ids in sorted(by_project.items()):
        project_results = [r for r in results if r.get("task_id") in task_ids]
        if not project_results:
            continue

        first_task = tasks.get(task_ids[0], {})
        language = first_task.get("language", "").capitalize() or project_name

        page = generate_task_detail_page(
            project_name.capitalize(),
            language,
            sorted(task_ids),
            project_results,
            tasks,
            judge_results,
            charts_dir=charts_dir,
        )
        filename = f"{project_name}.md"
        (odir / filename).write_text(page, encoding="utf-8")
        generated.append(filename)

    # Trends page (when multiple runs exist or explicitly requested).
    if include_trends or len(all_runs) > 1:
        trends = generate_trends_page(all_runs, charts_dir=charts_dir)
        (odir / "trends.md").write_text(trends, encoding="utf-8")
        generated.append("trends.md")

    logger.info("Generated %d pages in %s", len(generated), odir)
    return generated
