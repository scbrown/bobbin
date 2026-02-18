"""Search weight calibration: sweep parameter configs against eval tasks.

Measures how well bobbin's context assembly finds ground truth files
across different parameter configurations (semantic_weight, doc_demotion,
rrf_k). No LLM calls — pure search-quality measurement.

Usage (via eval CLI)::

    bobbin-eval calibrate --tasks-dir tasks --repos ruff cargo
    bobbin-eval calibrate --task ruff-001 --semantic-weights 0.5,0.6,0.7,0.8
"""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from itertools import product
from pathlib import Path
from typing import Any

from runner.task_loader import load_all_tasks, load_task_by_id
from runner.workspace import clone_repo, checkout_parent

logger = logging.getLogger(__name__)


class CalibrationError(Exception):
    """Raised when calibration setup fails."""


@dataclass
class ParamConfig:
    """A single parameter configuration to test."""

    semantic_weight: float
    doc_demotion: float
    rrf_k: float

    @property
    def label(self) -> str:
        return f"sw={self.semantic_weight:.2f}_dd={self.doc_demotion:.2f}_k={self.rrf_k:.1f}"


@dataclass
class SearchResult:
    """Result of a single search probe."""

    task_id: str
    config: ParamConfig
    returned_files: list[str]
    ground_truth_files: list[str]
    precision: float
    recall: float
    f1: float
    top_semantic_score: float
    total_files: int
    total_chunks: int
    duration_ms: float


@dataclass
class CalibrationReport:
    """Aggregated calibration results."""

    configs: list[ParamConfig]
    results: list[SearchResult] = field(default_factory=list)

    def summary_by_config(self) -> list[dict[str, Any]]:
        """Aggregate metrics per config."""
        summaries = []
        for config in self.configs:
            config_results = [r for r in self.results if r.config.label == config.label]
            if not config_results:
                continue
            n = len(config_results)
            avg_precision = sum(r.precision for r in config_results) / n
            avg_recall = sum(r.recall for r in config_results) / n
            avg_f1 = sum(r.f1 for r in config_results) / n
            avg_top_score = sum(r.top_semantic_score for r in config_results) / n
            avg_duration = sum(r.duration_ms for r in config_results) / n

            summaries.append({
                "config": config.label,
                "semantic_weight": config.semantic_weight,
                "doc_demotion": config.doc_demotion,
                "rrf_k": config.rrf_k,
                "tasks": n,
                "avg_precision": round(avg_precision, 4),
                "avg_recall": round(avg_recall, 4),
                "avg_f1": round(avg_f1, 4),
                "avg_top_semantic_score": round(avg_top_score, 4),
                "avg_duration_ms": round(avg_duration, 1),
            })
        return sorted(summaries, key=lambda s: s["avg_f1"], reverse=True)

    def to_markdown(self) -> str:
        """Generate markdown report."""
        lines = ["# Bobbin Search Weight Calibration Report\n"]
        summaries = self.summary_by_config()
        if not summaries:
            lines.append("No results collected.\n")
            return "\n".join(lines)

        # Summary table
        lines.append("## Summary (sorted by F1)\n")
        lines.append(
            "| Config | Semantic | DocDem | RRF k | Tasks | Precision | Recall | F1 | Top Score | Latency |"
        )
        lines.append(
            "|--------|----------|--------|-------|-------|-----------|--------|----|-----------|---------|"
        )
        for s in summaries:
            lines.append(
                f"| {s['config']} | {s['semantic_weight']:.2f} | {s['doc_demotion']:.2f} "
                f"| {s['rrf_k']:.0f} | {s['tasks']} | {s['avg_precision']:.3f} "
                f"| {s['avg_recall']:.3f} | **{s['avg_f1']:.3f}** "
                f"| {s['avg_top_semantic_score']:.3f} | {s['avg_duration_ms']:.0f}ms |"
            )

        # Best config
        if summaries:
            best = summaries[0]
            lines.append(f"\n## Best Config: {best['config']}")
            lines.append(f"- F1: {best['avg_f1']:.4f}")
            lines.append(f"- Precision: {best['avg_precision']:.4f}")
            lines.append(f"- Recall: {best['avg_recall']:.4f}")

        # Per-task detail
        lines.append("\n## Per-Task Results\n")
        tasks = sorted(set(r.task_id for r in self.results))
        for task_id in tasks:
            lines.append(f"\n### {task_id}\n")
            task_results = [r for r in self.results if r.task_id == task_id]
            if not task_results:
                continue
            gt_files = task_results[0].ground_truth_files
            lines.append(f"Ground truth files: {', '.join(gt_files)}\n")
            lines.append("| Config | Precision | Recall | F1 | Returned Files |")
            lines.append("|--------|-----------|--------|----|----------------|")
            for r in sorted(task_results, key=lambda x: x.f1, reverse=True):
                returned = ", ".join(r.returned_files[:5])
                if len(r.returned_files) > 5:
                    returned += f" (+{len(r.returned_files) - 5} more)"
                lines.append(
                    f"| {r.config.label} | {r.precision:.3f} | {r.recall:.3f} "
                    f"| {r.f1:.3f} | {returned} |"
                )

        return "\n".join(lines)


def _find_bobbin() -> str:
    """Find the bobbin binary."""
    found = shutil.which("bobbin")
    if found:
        return found
    cargo_bin = Path.home() / ".cargo" / "bin" / "bobbin"
    if cargo_bin.exists():
        return str(cargo_bin)
    raise CalibrationError("bobbin binary not found")


def _get_ground_truth_files(workspace: Path, commit: str) -> list[str]:
    """Get the list of files changed in the target commit."""
    result = subprocess.run(
        ["git", "diff-tree", "--no-commit-id", "--name-only", "-r", commit],
        cwd=workspace,
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    return [f.strip() for f in result.stdout.splitlines() if f.strip()]


def _extract_query(task: dict) -> str:
    """Build a search query from the task description.

    Uses the first sentence or two of the description, which is what
    an agent would receive as context.
    """
    desc = task["description"].strip()
    # Take first 200 chars as a reasonable query
    if len(desc) > 200:
        # Try to cut at sentence boundary
        cutoff = desc[:200].rfind(". ")
        if cutoff > 80:
            desc = desc[: cutoff + 1]
        else:
            desc = desc[:200]
    return desc


def _run_bobbin_context(
    workspace: Path,
    query: str,
    config: ParamConfig,
    bobbin: str,
) -> dict[str, Any]:
    """Run bobbin context with specific parameters and return parsed JSON."""
    cmd = [
        bobbin,
        "context",
        "--json",
        "--semantic-weight",
        str(config.semantic_weight),
        "--doc-demotion",
        str(config.doc_demotion),
        "--rrf-k",
        str(config.rrf_k),
        query,
    ]
    t0 = time.monotonic()
    try:
        result = subprocess.run(
            cmd,
            cwd=workspace,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except subprocess.TimeoutExpired:
        return {"error": "timeout", "duration_ms": (time.monotonic() - t0) * 1000}

    duration_ms = (time.monotonic() - t0) * 1000

    if result.returncode != 0:
        return {
            "error": f"exit {result.returncode}: {result.stderr.strip()[:200]}",
            "duration_ms": duration_ms,
        }

    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": "invalid JSON output", "duration_ms": duration_ms}

    data["duration_ms"] = duration_ms
    return data


def _compute_file_metrics(
    returned_files: list[str], ground_truth_files: list[str]
) -> tuple[float, float, float]:
    """Compute precision, recall, F1 for file overlap."""
    if not returned_files and not ground_truth_files:
        return 1.0, 1.0, 1.0
    if not returned_files:
        return 0.0, 0.0, 0.0
    if not ground_truth_files:
        return 0.0, 0.0, 0.0

    returned_set = set(returned_files)
    gt_set = set(ground_truth_files)
    overlap = returned_set & gt_set

    precision = len(overlap) / len(returned_set) if returned_set else 0.0
    recall = len(overlap) / len(gt_set) if gt_set else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) > 0 else 0.0
    return precision, recall, f1


def setup_workspace(
    task: dict, tmpdir: str, *, index_timeout: int = 600
) -> tuple[Path, list[str]]:
    """Clone repo, checkout parent, index with bobbin.

    Returns (workspace_path, ground_truth_files).
    """
    bobbin = _find_bobbin()
    repo = task["repo"]
    commit = task["commit"]

    # Clone and checkout parent
    ws = clone_repo(repo, tmpdir)
    parent_hash = checkout_parent(ws, commit)

    # Get ground truth files
    gt_files = _get_ground_truth_files(ws, commit)
    if not gt_files:
        raise CalibrationError(f"No files changed in commit {commit}")

    # Init and index
    logger.info("Initializing bobbin in %s", ws)
    subprocess.run(
        [bobbin, "init"],
        cwd=ws,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )

    logger.info("Indexing workspace %s", ws)
    subprocess.run(
        [bobbin, "index"],
        cwd=ws,
        check=True,
        capture_output=True,
        text=True,
        timeout=index_timeout,
    )

    return ws, gt_files


def run_calibration(
    tasks: list[dict],
    configs: list[ParamConfig],
    *,
    index_timeout: int = 600,
    keep_workspaces: bool = False,
) -> CalibrationReport:
    """Run calibration: for each task, index once then sweep all configs."""
    bobbin = _find_bobbin()
    report = CalibrationReport(configs=configs)

    # Group tasks by repo so we only clone/index each repo once
    by_repo: dict[str, list[dict]] = {}
    for task in tasks:
        by_repo.setdefault(task["repo"], []).append(task)

    for repo, repo_tasks in by_repo.items():
        logger.info("=== Repo: %s (%d tasks) ===", repo, len(repo_tasks))

        with tempfile.TemporaryDirectory(prefix="bobbin-cal-") as tmpdir:
            # Set up workspace for first task (clone + index)
            first_task = repo_tasks[0]
            try:
                ws, _ = setup_workspace(
                    first_task, tmpdir, index_timeout=index_timeout
                )
            except (CalibrationError, subprocess.CalledProcessError) as exc:
                logger.error("Failed to set up workspace for %s: %s", repo, exc)
                continue

            # Now sweep each task and config
            for task in repo_tasks:
                task_id = task["id"]
                commit = task["commit"]

                # Checkout parent of this task's commit
                try:
                    checkout_parent(ws, commit)
                except Exception as exc:
                    logger.warning("Skipping %s: %s", task_id, exc)
                    continue

                # Re-index for this commit state
                try:
                    subprocess.run(
                        [bobbin, "index"],
                        cwd=ws,
                        check=True,
                        capture_output=True,
                        text=True,
                        timeout=index_timeout,
                    )
                except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
                    logger.warning("Re-index failed for %s: %s", task_id, exc)
                    continue

                gt_files = _get_ground_truth_files(ws, commit)
                if not gt_files:
                    logger.warning("No ground truth files for %s", task_id)
                    continue

                query = _extract_query(task)
                logger.info("  Task %s: %d ground truth files, query=%s...",
                            task_id, len(gt_files), query[:60])

                for config in configs:
                    result_data = _run_bobbin_context(ws, query, config, bobbin)

                    if "error" in result_data:
                        logger.warning(
                            "    %s: error: %s", config.label, result_data["error"]
                        )
                        continue

                    # Extract returned file paths
                    returned_files = [
                        f["path"] for f in result_data.get("files", [])
                    ]
                    summary = result_data.get("summary", {})

                    precision, recall, f1 = _compute_file_metrics(
                        returned_files, gt_files
                    )

                    result = SearchResult(
                        task_id=task_id,
                        config=config,
                        returned_files=returned_files,
                        ground_truth_files=gt_files,
                        precision=precision,
                        recall=recall,
                        f1=f1,
                        top_semantic_score=summary.get("top_semantic_score", 0.0),
                        total_files=summary.get("total_files", 0),
                        total_chunks=summary.get("total_chunks", 0),
                        duration_ms=result_data.get("duration_ms", 0.0),
                    )
                    report.results.append(result)

                    logger.info(
                        "    %s: P=%.3f R=%.3f F1=%.3f (%d files returned)",
                        config.label,
                        precision,
                        recall,
                        f1,
                        len(returned_files),
                    )

    return report


def build_param_grid(
    semantic_weights: list[float] | None = None,
    doc_demotions: list[float] | None = None,
    rrf_ks: list[float] | None = None,
) -> list[ParamConfig]:
    """Build parameter grid from value lists."""
    sw = semantic_weights or [0.5, 0.6, 0.7, 0.8, 0.9]
    dd = doc_demotions or [0.3, 0.5, 0.7, 1.0]
    k = rrf_ks or [20.0, 40.0, 60.0, 80.0]

    return [
        ParamConfig(semantic_weight=s, doc_demotion=d, rrf_k=r)
        for s, d, r in product(sw, dd, k)
    ]
