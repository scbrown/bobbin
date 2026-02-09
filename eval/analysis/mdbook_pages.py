"""Generate mdbook-compatible markdown pages from eval results.

Reads result JSON files and task YAML definitions, then produces markdown
pages with inline SVG charts for the bobbin documentation site.
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

import yaml

from analysis.svg_charts import (
    GREEN,
    PURPLE,
    grouped_bar_chart,
    horizontal_bar,
)

logger = logging.getLogger(__name__)


class PageGenError(Exception):
    """Raised when page generation fails."""


def _load_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load all completed result JSON files."""
    results = []
    for f in sorted(results_dir.glob("*.json")):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            if isinstance(data, dict) and data.get("status") == "completed":
                results.append(data)
        except (json.JSONDecodeError, OSError):
            pass
    return results


def _load_judge_results(results_dir: Path) -> list[dict[str, Any]]:
    """Load judge results if available."""
    judge_file = results_dir / "judge_results.json"
    if not judge_file.exists():
        return []
    try:
        data = json.loads(judge_file.read_text(encoding="utf-8"))
        return data if isinstance(data, list) else []
    except (json.JSONDecodeError, OSError):
        return []


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

    return {
        "count": len(results),
        "test_pass_rate": pass_rate,
        "avg_file_precision": _safe_avg(precisions),
        "avg_file_recall": _safe_avg(recalls),
        "avg_f1": _safe_avg(f1s),
        "avg_duration_seconds": _safe_avg(durations),
    }


def _pick_best(runs: list[dict]) -> dict | None:
    """Pick best attempt: prefer passing, then highest F1."""
    if not runs:
        return None
    passing = [r for r in runs if r.get("test_result", {}).get("passed")]
    pool = passing if passing else runs
    return max(pool, key=lambda r: r.get("diff_result", {}).get("f1", 0.0))


def _delta_str(baseline: float, treatment: float) -> str:
    """Format delta with arrow indicator."""
    diff = treatment - baseline
    if abs(diff) < 0.001:
        return "—"
    arrow = "↑" if diff > 0 else "↓"
    return f"{arrow} {abs(diff):+.1%}"[2:]  # strip the +


def _format_pct(val: float) -> str:
    return f"{val * 100:.1f}%"


def _format_duration(seconds: float) -> str:
    if seconds < 60:
        return f"{seconds:.0f}s"
    return f"{seconds / 60:.1f}m"


# -- Page generators --


def generate_summary_page(
    results: list[dict],
    judge_results: list[dict],
    tasks: dict[str, dict],
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

    metrics = [
        ("Runs", "count", False),
        ("Test Pass Rate", "test_pass_rate", True),
        ("Avg Precision", "avg_file_precision", True),
        ("Avg Recall", "avg_file_recall", True),
        ("Avg F1", "avg_f1", True),
        ("Avg Duration", "avg_duration_seconds", False),
    ]

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
            elif key == "avg_duration_seconds" and vals[0] > 0:
                pct = (vals[1] - vals[0]) / vals[0] * 100
                row += f" {pct:+.0f}% |"
            else:
                row += " |"
        lines.append(row)

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
        lines.append('<div class="eval-chart">')
        lines.append("")
        lines.append(grouped_bar_chart(chart_groups, title="F1 Score Comparison"))
        lines.append("")
        lines.append("</div>")
        lines.append("")

    # Per-task mini-table.
    lines.append("## Per-Task Results")
    lines.append("")
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
            lines.append(
                f"| {task_id} | {lang} | {diff} | {a} "
                f"| {pass_str} | {_format_pct(s['avg_file_precision'])} "
                f"| {_format_pct(s['avg_file_recall'])} | {_format_pct(s['avg_f1'])} "
                f"| {_format_duration(s['avg_duration_seconds'])} |"
            )

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
            for lang, stats in sorted_langs[:10]:
                lines.append(
                    f"| {lang} | {stats.get('files', 0):,} | {stats.get('code', 0):,} "
                    f"| {stats.get('comments', 0):,} | {stats.get('blanks', 0):,} "
                    f"| {stats.get('lines', 0):,} |"
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

    return "\n".join(lines) + "\n"


def generate_task_detail_page(
    project_name: str,
    language: str,
    task_ids: list[str],
    results: list[dict],
    tasks: dict[str, dict],
    judge_results: list[dict],
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
            lines.append("<details>")
            lines.append("<summary>Task prompt</summary>")
            lines.append("")
            lines.append(f"> {desc}")
            lines.append("")
            lines.append("</details>")
            lines.append("")

        # Results table.
        task_by_approach = _group_by_approach(task_results)
        approaches = sorted(task_by_approach.keys())

        lines.append("| Approach | Tests Pass | Precision | Recall | F1 | Duration |")
        lines.append("|----------|:----------:|:---------:|:------:|:--:|:--------:|")

        for a in approaches:
            s = _compute_approach_stats(task_by_approach[a])
            lines.append(
                f"| {a} | {_format_pct(s['test_pass_rate'])} "
                f"| {_format_pct(s['avg_file_precision'])} "
                f"| {_format_pct(s['avg_file_recall'])} "
                f"| {_format_pct(s['avg_f1'])} "
                f"| {_format_duration(s['avg_duration_seconds'])} |"
            )

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

        # Mini bar chart for this task.
        if len(approaches) >= 2:
            chart_values = {}
            for a in approaches:
                s = _compute_approach_stats(task_by_approach[a])
                chart_values[a] = s["avg_f1"]
            chart = grouped_bar_chart(
                [{"label": task_id, "values": chart_values}],
                width=300,
                height=180,
                title=f"{task_id} F1 Score",
            )
            lines.append('<div class="eval-chart">')
            lines.append("")
            lines.append(chart)
            lines.append("")
            lines.append("</div>")
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

    return "\n".join(lines) + "\n"


def generate_all_pages(
    results_dir: str,
    tasks_dir: str,
    output_dir: str,
) -> list[str]:
    """Generate all mdbook pages from eval results.

    Returns a list of generated file paths (relative to output_dir).
    """
    rdir = Path(results_dir)
    tdir = Path(tasks_dir)
    odir = Path(output_dir)
    odir.mkdir(parents=True, exist_ok=True)

    results = _load_results(rdir)
    if not results:
        raise PageGenError(f"No completed results in {rdir}")

    tasks = _load_tasks(tdir)
    judge_results = _load_judge_results(rdir)

    generated = []

    # Summary page.
    summary = generate_summary_page(results, judge_results, tasks)
    (odir / "summary.md").write_text(summary, encoding="utf-8")
    generated.append("summary.md")

    # Projects page.
    projects = generate_project_page(results, tasks)
    (odir / "projects.md").write_text(projects, encoding="utf-8")
    generated.append("projects.md")

    # Per-project detail pages.
    # Group tasks by repo → project.
    by_project: dict[str, list[str]] = {}
    for task_id, task in tasks.items():
        repo = task.get("repo", "")
        project_name = repo.split("/")[-1] if repo else "unknown"
        by_project.setdefault(project_name, []).append(task_id)

    for project_name, task_ids in sorted(by_project.items()):
        # Filter results for this project's tasks.
        project_results = [r for r in results if r.get("task_id") in task_ids]
        if not project_results:
            continue

        # Determine language from first task.
        first_task = tasks.get(task_ids[0], {})
        language = first_task.get("language", "").capitalize() or project_name

        page = generate_task_detail_page(
            project_name.capitalize(),
            language,
            sorted(task_ids),
            project_results,
            tasks,
            judge_results,
        )
        filename = f"{project_name}.md"
        (odir / filename).write_text(page, encoding="utf-8")
        generated.append(filename)

    logger.info("Generated %d pages in %s", len(generated), odir)
    return generated
