#!/usr/bin/env python3
"""Temporal decay parameter sweeps for the bobbin paper.

Runs controlled 1D sweeps of coupling_depth and recency_weight against
eval tasks, holding all other parameters constant. Uses the calibrate.py
approach: search-only probes with bobbin context (no LLM calls).

Outputs JSON results suitable for paper figure generation.

Usage::

    python3 scripts/temporal_sweep.py
    python3 scripts/temporal_sweep.py --repos ruff cargo flask
    python3 scripts/temporal_sweep.py --output results/temporal-sweep.json
    python3 scripts/temporal_sweep.py --coupling-only
    python3 scripts/temporal_sweep.py --recency-only
"""

from __future__ import annotations

import argparse
import json
import logging
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

# Add eval root to path so we can import runner modules
_EVAL_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_EVAL_ROOT))

from runner.task_loader import load_all_tasks  # noqa: E402
from runner.workspace import clone_repo, checkout_parent  # noqa: E402

logger = logging.getLogger(__name__)

# Sweep parameters (from bead aegis-o1jqap.4)
COUPLING_DEPTHS = [0, 100, 500, 1000, 5000]
RECENCY_WEIGHTS = [0.0, 0.1, 0.2, 0.3, 0.5]

# Representative task subset — 2 per repo for breadth, chosen for varied
# difficulty and file counts. Override with --repos.
DEFAULT_REPOS = ["django/django", "typst/typst", "golang/go",
                 "pandas-dev/pandas", "nushell/nushell"]


@dataclass
class ProbeResult:
    """Result of a single search probe."""
    task_id: str
    repo: str
    sweep_param: str  # "coupling_depth" or "recency_weight"
    sweep_value: float
    returned_files: list[str]
    ground_truth_files: list[str]
    precision: float
    recall: float
    f1: float
    duration_ms: float
    total_files: int
    total_chunks: int
    error: str | None = None


@dataclass
class SweepResults:
    """Aggregated sweep results."""
    sweep_type: str
    sweep_values: list[float]
    probes: list[ProbeResult] = field(default_factory=list)
    started_at: str = ""
    finished_at: str = ""
    total_duration_s: float = 0.0


def find_bobbin() -> str:
    """Find the bobbin binary."""
    found = shutil.which("bobbin")
    if found:
        return found
    cargo_bin = Path.home() / ".cargo" / "bin" / "bobbin"
    if cargo_bin.exists():
        return str(cargo_bin)
    raise RuntimeError("bobbin binary not found")


def get_ground_truth_files(workspace: Path, commit: str) -> list[str]:
    """Get files changed in the target commit."""
    result = subprocess.run(
        ["git", "diff-tree", "--no-commit-id", "--name-only", "-r", commit],
        cwd=workspace, capture_output=True, text=True, check=True, timeout=30,
    )
    return [f.strip() for f in result.stdout.splitlines() if f.strip()]


def extract_query(task: dict) -> str:
    """Build a search query from the task description."""
    desc = task["description"].strip()
    if len(desc) > 200:
        cutoff = desc[:200].rfind(". ")
        if cutoff > 80:
            desc = desc[:cutoff + 1]
        else:
            desc = desc[:200]
    return desc


def run_bobbin_context(
    workspace: Path, query: str, bobbin: str,
) -> dict[str, Any]:
    """Run bobbin context and return parsed JSON.

    Uses config.toml / calibration.json values. Modify config.toml before
    calling to override parameters.
    """
    cmd = [bobbin, "context", "--json"]
    cmd.append(query)
    t0 = time.monotonic()
    try:
        result = subprocess.run(
            cmd, cwd=workspace, capture_output=True, text=True, timeout=120,
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


def compute_file_metrics(
    returned_files: list[str], ground_truth_files: list[str],
) -> tuple[float, float, float]:
    """Compute precision, recall, F1 for file overlap."""
    if not returned_files and not ground_truth_files:
        return 1.0, 1.0, 1.0
    if not returned_files or not ground_truth_files:
        return 0.0, 0.0, 0.0

    returned_set = set(returned_files)
    gt_set = set(ground_truth_files)
    overlap = returned_set & gt_set

    precision = len(overlap) / len(returned_set)
    recall = len(overlap) / len(gt_set)
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) > 0 else 0.0
    return precision, recall, f1


def modify_config_toml(workspace: Path, section: str, key: str, value: Any) -> None:
    """Modify a value in .bobbin/config.toml."""
    config_path = workspace / ".bobbin" / "config.toml"
    content = config_path.read_text()

    # Format value for TOML
    if isinstance(value, bool):
        toml_val = "true" if value else "false"
    elif isinstance(value, int):
        toml_val = str(value)
    elif isinstance(value, float):
        toml_val = f"{value:.6g}"
        if "." not in toml_val:
            toml_val += ".0"
    else:
        toml_val = str(value)

    lines = content.splitlines(keepends=True)
    section_header = f"[{section}]"
    in_section = False
    section_start = -1
    section_end = len(lines)
    key_line_idx = -1

    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped == section_header:
            in_section = True
            section_start = i
            continue
        if in_section and stripped.startswith("[") and stripped.endswith("]"):
            section_end = i
            break
        if in_section and (stripped.startswith(f"{key} ") or stripped.startswith(f"{key}=")):
            key_line_idx = i

    if key_line_idx >= 0:
        lines[key_line_idx] = f"{key} = {toml_val}\n"
    elif section_start >= 0:
        lines.insert(section_end, f"{key} = {toml_val}\n")
    else:
        lines.append(f"\n{section_header}\n{key} = {toml_val}\n")

    config_path.write_text("".join(lines))



def setup_workspace(
    task: dict, tmpdir: str, bobbin: str, *, index_timeout: int = 1800,
    coupling_depth: int | None = None,
) -> tuple[Path, list[str]]:
    """Clone repo, checkout parent, init+index with bobbin.

    If coupling_depth is set, override the default (5000) before indexing.
    Use coupling_depth=0 when the caller will re-index at different depths
    to avoid wasting time building a coupling table that gets overwritten.
    """
    repo = task["repo"]
    commit = task["commit"]

    ws = clone_repo(repo, tmpdir)
    checkout_parent(ws, commit)

    gt_files = get_ground_truth_files(ws, commit)
    if not gt_files:
        raise RuntimeError(f"No files changed in commit {commit}")

    logger.info("Initializing bobbin in %s", ws)
    subprocess.run(
        [bobbin, "init"], cwd=ws, check=True,
        capture_output=True, text=True, timeout=30,
    )

    # Enable GPU for embedding — default config has gpu=false
    modify_config_toml(ws, "embedding", "gpu", True)

    if coupling_depth is not None:
        modify_config_toml(ws, "git", "coupling_depth", coupling_depth)

    logger.info("Indexing workspace %s", ws)
    subprocess.run(
        [bobbin, "index", "--skip-calibrate"], cwd=ws, check=True,
        capture_output=True, text=True, timeout=index_timeout,
    )

    return ws, gt_files


def probe_tasks(
    workspace: Path, tasks: list[dict], bobbin: str,
    sweep_param: str, sweep_value: float,
) -> list[ProbeResult]:
    """Run context probes for all tasks at the current config state."""
    ws_prefix = str(workspace) + "/"
    results = []

    for task in tasks:
        commit = task["commit"]
        try:
            checkout_parent(workspace, commit)
        except Exception as exc:
            logger.warning("Skipping %s: %s", task["id"], exc)
            results.append(ProbeResult(
                task_id=task["id"], repo=task["repo"],
                sweep_param=sweep_param, sweep_value=sweep_value,
                returned_files=[], ground_truth_files=[],
                precision=0, recall=0, f1=0,
                duration_ms=0, total_files=0, total_chunks=0,
                error=str(exc),
            ))
            continue

        gt_files = get_ground_truth_files(workspace, commit)
        if not gt_files:
            logger.warning("No ground truth files for %s", task["id"])
            continue

        query = extract_query(task)
        data = run_bobbin_context(workspace, query, bobbin)

        if "error" in data:
            logger.warning("  %s: error: %s", task["id"], data["error"])
            results.append(ProbeResult(
                task_id=task["id"], repo=task["repo"],
                sweep_param=sweep_param, sweep_value=sweep_value,
                returned_files=[], ground_truth_files=gt_files,
                precision=0, recall=0, f1=0,
                duration_ms=data.get("duration_ms", 0),
                total_files=0, total_chunks=0,
                error=data["error"],
            ))
            continue

        returned_files = []
        for f in data.get("files", []):
            p = f["path"]
            if p.startswith(ws_prefix):
                p = p[len(ws_prefix):]
            returned_files.append(p)

        summary = data.get("summary", {})
        precision, recall, f1 = compute_file_metrics(returned_files, gt_files)

        results.append(ProbeResult(
            task_id=task["id"], repo=task["repo"],
            sweep_param=sweep_param, sweep_value=sweep_value,
            returned_files=returned_files,
            ground_truth_files=gt_files,
            precision=precision, recall=recall, f1=f1,
            duration_ms=data.get("duration_ms", 0),
            total_files=summary.get("total_files", 0),
            total_chunks=summary.get("total_chunks", 0),
        ))

        logger.info("  %s [%s=%s]: P=%.3f R=%.3f F1=%.3f (%d files)",
                     task["id"], sweep_param, sweep_value,
                     precision, recall, f1, len(returned_files))

    return results


def run_coupling_depth_sweep(
    tasks_by_repo: dict[str, list[dict]], bobbin: str,
) -> SweepResults:
    """Sweep coupling_depth, re-indexing at each depth."""
    results = SweepResults(
        sweep_type="coupling_depth",
        sweep_values=[float(d) for d in COUPLING_DEPTHS],
        started_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    )
    t0 = time.monotonic()

    for repo, tasks in tasks_by_repo.items():
        logger.info("=== Coupling depth sweep: %s (%d tasks) ===", repo, len(tasks))

        with tempfile.TemporaryDirectory(prefix="bobbin-sweep-cd-") as tmpdir:
            try:
                # Use first sweep depth for initial index to avoid wasted work
                ws, _ = setup_workspace(tasks[0], tmpdir, bobbin,
                                        coupling_depth=COUPLING_DEPTHS[0])
            except Exception as exc:
                logger.error("Failed to set up %s: %s", repo, exc)
                continue

            for depth in COUPLING_DEPTHS:
                logger.info("  coupling_depth=%d", depth)

                # Modify config and re-index to rebuild coupling table
                modify_config_toml(ws, "git", "coupling_depth", depth)

                # Remove any calibration.json to avoid interference
                cal_path = ws / ".bobbin" / "calibration.json"
                if cal_path.exists():
                    cal_path.unlink()

                try:
                    subprocess.run(
                        [bobbin, "index", "--force", "--skip-calibrate"],
                        cwd=ws, check=True,
                        capture_output=True, text=True, timeout=1800,
                    )
                except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
                    logger.error("  Re-index failed at depth=%d: %s", depth, exc)
                    continue

                probes = probe_tasks(ws, tasks, bobbin, "coupling_depth", float(depth))
                results.probes.extend(probes)

    results.finished_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    results.total_duration_s = round(time.monotonic() - t0, 1)
    return results


def run_recency_weight_sweep(
    tasks_by_repo: dict[str, list[dict]], bobbin: str,
) -> SweepResults:
    """Sweep recency_weight via config.toml modification (no re-indexing)."""
    results = SweepResults(
        sweep_type="recency_weight",
        sweep_values=RECENCY_WEIGHTS,
        started_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    )
    t0 = time.monotonic()

    for repo, tasks in tasks_by_repo.items():
        logger.info("=== Recency weight sweep: %s (%d tasks) ===", repo, len(tasks))

        with tempfile.TemporaryDirectory(prefix="bobbin-sweep-rw-") as tmpdir:
            try:
                ws, _ = setup_workspace(tasks[0], tmpdir, bobbin)
            except Exception as exc:
                logger.error("Failed to set up %s: %s", repo, exc)
                continue

            for weight in RECENCY_WEIGHTS:
                logger.info("  recency_weight=%.1f", weight)

                # Set recency_weight in config.toml (no CLI flag available)
                modify_config_toml(ws, "search", "recency_weight", weight)

                # Remove calibration.json to avoid it overriding config
                cal_path = ws / ".bobbin" / "calibration.json"
                if cal_path.exists():
                    cal_path.unlink()

                probes = probe_tasks(ws, tasks, bobbin, "recency_weight", weight)
                results.probes.extend(probes)

    results.finished_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    results.total_duration_s = round(time.monotonic() - t0, 1)
    return results


def select_tasks(
    all_tasks: list[dict], repos: list[str] | None, max_per_repo: int = 2,
) -> dict[str, list[dict]]:
    """Select a representative subset of tasks grouped by repo."""
    # Filter to requested repos
    if repos:
        repo_set = set(repos)
        filtered = [t for t in all_tasks if t["repo"] in repo_set]
    else:
        # Use DEFAULT_REPOS
        repo_set = set(DEFAULT_REPOS)
        filtered = [t for t in all_tasks if t["repo"] in repo_set]

    # Group by repo, take first N per repo
    by_repo: dict[str, list[dict]] = {}
    for task in filtered:
        repo = task["repo"]
        if repo not in by_repo:
            by_repo[repo] = []
        if len(by_repo[repo]) < max_per_repo:
            by_repo[repo].append(task)

    return by_repo


def main() -> None:
    parser = argparse.ArgumentParser(description="Temporal decay parameter sweeps")
    parser.add_argument("--repos", nargs="*", help="Repos to sweep (default: 5 representative)")
    parser.add_argument("--tasks-dir", default=str(_EVAL_ROOT / "tasks"),
                        help="Directory containing task YAML files")
    parser.add_argument("--output", default=str(_EVAL_ROOT / "results" / "temporal-sweep.json"),
                        help="Output JSON file")
    parser.add_argument("--max-per-repo", type=int, default=2,
                        help="Max tasks per repo (default: 2)")
    parser.add_argument("--coupling-only", action="store_true",
                        help="Only run coupling depth sweep")
    parser.add_argument("--recency-only", action="store_true",
                        help="Only run recency weight sweep")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)s %(message)s",
        datefmt="%H:%M:%S",
    )

    bobbin = find_bobbin()
    logger.info("Using bobbin: %s", bobbin)

    all_tasks = load_all_tasks(args.tasks_dir)
    tasks_by_repo = select_tasks(all_tasks, args.repos, args.max_per_repo)

    total_tasks = sum(len(v) for v in tasks_by_repo.values())
    logger.info("Selected %d tasks across %d repos", total_tasks, len(tasks_by_repo))
    for repo, tasks in tasks_by_repo.items():
        logger.info("  %s: %s", repo, [t["id"] for t in tasks])

    output: dict[str, Any] = {"metadata": {
        "repos": list(tasks_by_repo.keys()),
        "tasks_per_repo": {repo: [t["id"] for t in tasks]
                           for repo, tasks in tasks_by_repo.items()},
        "coupling_depths": COUPLING_DEPTHS,
        "recency_weights": RECENCY_WEIGHTS,
    }}

    if not args.recency_only:
        logger.info("=== Starting coupling depth sweep ===")
        cd_results = run_coupling_depth_sweep(tasks_by_repo, bobbin)
        output["coupling_depth"] = asdict(cd_results)
        logger.info("Coupling depth sweep: %d probes in %.0fs",
                     len(cd_results.probes), cd_results.total_duration_s)

    if not args.coupling_only:
        logger.info("=== Starting recency weight sweep ===")
        rw_results = run_recency_weight_sweep(tasks_by_repo, bobbin)
        output["recency_weight"] = asdict(rw_results)
        logger.info("Recency weight sweep: %d probes in %.0fs",
                     len(rw_results.probes), rw_results.total_duration_s)

    # Write output
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(output, indent=2))
    logger.info("Results written to %s", out_path)

    # Print summary table
    print("\n=== Summary ===")
    for sweep_key in ["coupling_depth", "recency_weight"]:
        if sweep_key not in output:
            continue
        sweep = output[sweep_key]
        print(f"\n{sweep['sweep_type']} sweep ({len(sweep['probes'])} probes):")
        # Aggregate by sweep_value
        by_value: dict[float, list[dict]] = {}
        for p in sweep["probes"]:
            v = p["sweep_value"]
            by_value.setdefault(v, []).append(p)

        print(f"  {'Value':>8s}  {'Avg F1':>7s}  {'Avg P':>7s}  {'Avg R':>7s}  {'N':>3s}")
        for v in sorted(by_value.keys()):
            probes = [p for p in by_value[v] if not p.get("error")]
            if not probes:
                print(f"  {v:>8.1f}  {'error':>7s}")
                continue
            n = len(probes)
            avg_f1 = sum(p["f1"] for p in probes) / n
            avg_p = sum(p["precision"] for p in probes) / n
            avg_r = sum(p["recall"] for p in probes) / n
            print(f"  {v:>8.1f}  {avg_f1:>7.3f}  {avg_p:>7.3f}  {avg_r:>7.3f}  {n:>3d}")


if __name__ == "__main__":
    main()
