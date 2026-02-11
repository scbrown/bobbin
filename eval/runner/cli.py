"""CLI entrypoint for the bobbin eval runner.

Provides commands for running evaluations, scoring results, and generating
reports. The runner orchestrates workspace setup, agent invocation, and
scoring for each task × approach × attempt combination.

Usage::

    bobbin-eval run-task ruff-001
    bobbin-eval run-all --tasks-dir tasks
    bobbin-eval score results/
    bobbin-eval report results/ --output report.md
"""

from __future__ import annotations

import json
import logging
import secrets
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

import click

from runner.task_loader import TaskLoadError, load_all_tasks, load_task_by_id

logger = logging.getLogger(__name__)

_EVAL_ROOT = Path(__file__).resolve().parent.parent
_DEFAULT_SETTINGS = _EVAL_ROOT / "settings-with-bobbin.json"
_NO_BOBBIN_SETTINGS = _EVAL_ROOT / "settings-no-bobbin.json"


def _setup_logging(verbose: bool) -> None:
    """Configure logging based on verbosity."""
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        datefmt="%H:%M:%S",
    )


def _resolve_tasks_dir(tasks_dir: str) -> Path:
    """Resolve tasks directory relative to the eval root."""
    p = Path(tasks_dir)
    if p.is_absolute():
        return p
    # Try relative to cwd first, then relative to eval/ directory.
    if p.is_dir():
        return p
    eval_root = Path(__file__).parent.parent
    candidate = eval_root / p
    if candidate.is_dir():
        return candidate
    return p  # Let downstream raise the error.


def _build_prompt(task: dict) -> str:
    """Build the agent prompt from a task definition."""
    repo = task["repo"]
    desc = task["description"].strip()
    test_cmd = task["test_command"]
    return (
        f"You are working on the {repo} project.\n\n"
        f"{desc}\n\n"
        f"Implement the fix. Run the test suite with `{test_cmd}` to verify."
    )


def _generate_run_id() -> str:
    """Generate a unique run ID in ``YYYYMMDD-HHMMSS-XXXX`` format.

    Uses UTC time and 4 random hex characters for uniqueness.
    """
    now = datetime.now(timezone.utc)
    return f"{now.strftime('%Y%m%d-%H%M%S')}-{secrets.token_hex(2)}"


def _save_result(result: dict, results_dir: Path, *, run_id: str | None = None) -> Path:
    """Save a single run result as a JSON file.

    Filename format: ``<task_id>_<approach>_<attempt>.json``

    When *run_id* is provided the file is saved under
    ``results/runs/<run_id>/`` and the ``run_id`` field is added to the
    result dict.  Otherwise the legacy flat layout is used.
    """
    if run_id is not None:
        result["run_id"] = run_id
        target_dir = results_dir / "runs" / run_id
    else:
        target_dir = results_dir

    target_dir.mkdir(parents=True, exist_ok=True)
    task_id = result.get("task_id", "unknown")
    approach = result.get("approach", "unknown")
    attempt = result.get("attempt", 0)
    filename = f"{task_id}_{approach}_{attempt}.json"
    path = target_dir / filename
    path.write_text(json.dumps(result, indent=2, default=str), encoding="utf-8")
    return path


def _write_manifest(
    results_dir: Path,
    run_id: str,
    *,
    started_at: str,
    completed_at: str,
    model: str,
    budget: float,
    timeout: int,
    index_timeout: int,
    attempts_per_approach: int,
    approaches: list[str],
    tasks: list[str],
    total_results: int,
    completed_results: int,
) -> Path:
    """Write ``manifest.json`` into the run directory."""
    run_dir = results_dir / "runs" / run_id
    run_dir.mkdir(parents=True, exist_ok=True)
    manifest = {
        "run_id": run_id,
        "started_at": started_at,
        "completed_at": completed_at,
        "agent_config": {
            "model": model,
            "budget_usd": budget,
            "timeout_seconds": timeout,
            "index_timeout_seconds": index_timeout,
        },
        "attempts_per_approach": attempts_per_approach,
        "approaches": approaches,
        "tasks": tasks,
        "total_results": total_results,
        "completed_results": completed_results,
    }
    path = run_dir / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return path


def _run_single(
    task: dict,
    approach: str,
    attempt: int,
    results_dir: Path,
    *,
    run_id: str | None = None,
    settings_file: str | None = None,
    model: str = "claude-opus-4-6",
    budget: float = 100.00,
    timeout: int = 3600,
    index_timeout: int = 600,
    skip_verify: bool = False,
) -> dict:
    """Execute a single task × approach × attempt evaluation run.

    Returns the result dict (also saved to results_dir).
    """
    from runner.agent_runner import run_agent
    from runner.bobbin_setup import setup_bobbin
    from runner.workspace import collect_loc_stats, diff_snapshot, setup_workspace, snapshot
    from scorer.diff_scorer import score_diff
    from scorer.test_scorer import run_tests

    # Resolve settings file to absolute path so it works from any cwd.
    if settings_file:
        settings_file = str(Path(settings_file).resolve())

    task_id = task["id"]
    click.echo(f"  [{task_id}] {approach} attempt {attempt + 1}")

    # 1. Setup workspace.
    with tempfile.TemporaryDirectory(prefix=f"bobbin-eval-{task_id}-") as tmpdir:
        click.echo("    Setting up workspace...")
        try:
            ws, parent = setup_workspace(
                task["repo"],
                task["commit"],
                task["test_command"],
                tmpdir,
                setup_command=task.get("setup_command"),
                verify=not skip_verify,
                setup_timeout=900,
            )
        except Exception as exc:
            click.echo(f"    Workspace setup failed: {exc}", err=True)
            result = {
                "task_id": task_id,
                "approach": approach,
                "attempt": attempt,
                "status": "workspace_error",
                "error": str(exc),
                "timestamp": datetime.now(timezone.utc).isoformat(),
            }
            _save_result(result, results_dir, run_id=run_id)
            return result

        # 2a. Collect LOC stats via tokei.
        project_metadata = None
        try:
            project_metadata = collect_loc_stats(ws)
        except Exception as exc:
            click.echo(f"    LOC stats collection failed (non-fatal): {exc}", err=True)

        # 2b. Bobbin setup (with-bobbin only).
        pre_agent_baseline = None
        bobbin_metadata = None
        if approach == "with-bobbin":
            click.echo("    Running bobbin init + index...")
            try:
                bobbin_metadata = setup_bobbin(str(ws), timeout=index_timeout)
            except Exception as exc:
                click.echo(f"    Bobbin setup failed: {exc}", err=True)
                result = {
                    "task_id": task_id,
                    "approach": approach,
                    "attempt": attempt,
                    "status": "bobbin_setup_error",
                    "error": str(exc),
                    "timestamp": datetime.now(timezone.utc).isoformat(),
                }
                _save_result(result, results_dir, run_id=run_id)
                return result
            # Snapshot after bobbin setup so diff scoring excludes bobbin
            # infrastructure files (.bobbin/, .gitignore changes).
            pre_agent_baseline = snapshot(ws)

        # 3. Run agent.
        prompt = _build_prompt(task)
        click.echo(f"    Running Claude Code (model={model}, budget=${budget:.2f})...")
        # Always pass a settings file to isolate from user's global hooks.
        # no-bobbin gets a clean settings file; with-bobbin gets the bobbin one.
        if approach == "with-bobbin":
            run_settings = settings_file
        else:
            run_settings = str(_NO_BOBBIN_SETTINGS) if _NO_BOBBIN_SETTINGS.exists() else None
        agent_result = run_agent(
            str(ws),
            prompt,
            settings_file=run_settings,
            model=model,
            max_budget_usd=budget,
            timeout=timeout,
        )

        # 4. Snapshot and score.
        click.echo("    Scoring...")
        snap = snapshot(ws)
        test_result = run_tests(str(ws), task["test_command"])
        diff_result = score_diff(
            str(ws), task["commit"],
            snapshot=snap,
            baseline=pre_agent_baseline,
        )

        # Capture diffs for post-hoc LLM judge comparison.
        diff_base = pre_agent_baseline or parent
        agent_diff = diff_snapshot(ws, diff_base, snap)
        ground_truth_diff = diff_snapshot(ws, parent, task["commit"])

        result = {
            "task_id": task_id,
            "approach": approach,
            "attempt": attempt,
            "status": "completed",
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "task": {
                "repo": task["repo"],
                "commit": task["commit"],
                "test_command": task["test_command"],
                "language": task.get("language"),
                "difficulty": task.get("difficulty"),
            },
            "agent_config": {
                "model": model,
                "budget_usd": budget,
                "timeout_seconds": timeout,
                "index_timeout_seconds": index_timeout,
            },
            "agent_result": {
                "exit_code": agent_result["exit_code"],
                "duration_seconds": agent_result["duration_seconds"],
                "timed_out": agent_result["timed_out"],
            },
            "test_result": {
                "passed": test_result["passed"],
                "total": test_result["total"],
                "failures": test_result["failures"],
                "parsed": test_result["parsed"],
            },
            "diff_result": {
                "file_precision": diff_result["file_precision"],
                "file_recall": diff_result["file_recall"],
                "f1": diff_result["f1"],
                "files_touched": diff_result["files_touched"],
                "ground_truth_files": diff_result["ground_truth_files"],
                "exact_file_match": diff_result["exact_file_match"],
            },
            "agent_diff": agent_diff,
            "ground_truth_diff": ground_truth_diff,
            "project_metadata": project_metadata,
            "bobbin_metadata": bobbin_metadata,
        }

        path = _save_result(result, results_dir, run_id=run_id)
        status = "PASS" if test_result["passed"] else "FAIL"
        click.echo(
            f"    {status} | precision={diff_result['file_precision']:.2f} "
            f"recall={diff_result['file_recall']:.2f} "
            f"f1={diff_result['f1']:.2f} | {agent_result['duration_seconds']:.0f}s"
        )
        click.echo(f"    Saved: {path.name}")

    return result


def _resolve_approaches(approaches: str) -> list[str]:
    """Expand 'both' into the two approach names."""
    if approaches == "both":
        return ["no-bobbin", "with-bobbin"]
    return [approaches]


@click.group()
@click.option("-v", "--verbose", is_flag=True, help="Enable debug logging.")
def cli(verbose: bool):
    """Bobbin evaluation framework — compare Claude Code with and without bobbin."""
    _setup_logging(verbose)


@cli.command()
@click.argument("task_id")
@click.option("--attempts", default=3, help="Number of attempts per approach.")
@click.option(
    "--approaches",
    default="both",
    type=click.Choice(["no-bobbin", "with-bobbin", "both"]),
    help="Which approaches to evaluate.",
)
@click.option("--tasks-dir", default="tasks", help="Directory containing task YAML files.")
@click.option("--results-dir", default="results", help="Directory to store result JSON files.")
@click.option("--settings-file", default=None, help="Claude Code settings file for with-bobbin.")
@click.option("--model", default="claude-opus-4-6", help="Claude model to use.")
@click.option("--budget", default=100.00, type=float, help="Max budget per run (USD).")
@click.option("--timeout", default=3600, type=int, help="Agent timeout in seconds.")
@click.option("--index-timeout", default=600, type=int, help="Bobbin index timeout in seconds.")
@click.option("--skip-verify", is_flag=True, help="Skip test verification at parent commit.")
def run_task(
    task_id: str,
    attempts: int,
    approaches: str,
    tasks_dir: str,
    results_dir: str,
    settings_file: str | None,
    model: str,
    budget: float,
    timeout: int,
    index_timeout: int,
    skip_verify: bool,
):
    """Run evaluation for a single task.

    TASK_ID is the task identifier (e.g., ruff-001).
    """
    tasks_path = _resolve_tasks_dir(tasks_dir)
    rdir = Path(results_dir)

    # Auto-resolve settings file for with-bobbin runs.
    if settings_file is None and _DEFAULT_SETTINGS.exists():
        settings_file = str(_DEFAULT_SETTINGS)

    try:
        task = load_task_by_id(task_id, tasks_path)
    except TaskLoadError as exc:
        click.echo(f"Error: {exc}", err=True)
        sys.exit(1)

    approach_list = _resolve_approaches(approaches)
    total = len(approach_list) * attempts
    run_id = _generate_run_id()
    started_at = datetime.now(timezone.utc).isoformat()
    click.echo(
        f"Running task {task_id}: {total} runs "
        f"({len(approach_list)} approaches × {attempts} attempts) "
        f"[run {run_id}]"
    )

    results = []
    for approach in approach_list:
        for attempt in range(attempts):
            result = _run_single(
                task,
                approach,
                attempt,
                rdir,
                run_id=run_id,
                settings_file=settings_file,
                model=model,
                budget=budget,
                timeout=timeout,
                index_timeout=index_timeout,
                skip_verify=skip_verify,
            )
            results.append(result)

    completed_at = datetime.now(timezone.utc).isoformat()
    completed_count = sum(1 for r in results if r.get("status") == "completed")
    _write_manifest(
        rdir,
        run_id,
        started_at=started_at,
        completed_at=completed_at,
        model=model,
        budget=budget,
        timeout=timeout,
        index_timeout=index_timeout,
        attempts_per_approach=attempts,
        approaches=approach_list,
        tasks=[task_id],
        total_results=len(results),
        completed_results=completed_count,
    )

    passed = sum(1 for r in results if r.get("test_result", {}).get("passed"))
    click.echo(f"\nDone: {passed}/{len(results)} runs passed tests [run {run_id}]")


@cli.command()
@click.option("--tasks-dir", default="tasks", help="Directory containing task YAML files.")
@click.option("--results-dir", default="results", help="Directory to store result JSON files.")
@click.option("--attempts", default=3, type=int, help="Number of attempts per approach.")
@click.option(
    "--approaches",
    default="both",
    type=click.Choice(["no-bobbin", "with-bobbin", "both"]),
)
@click.option("--settings-file", default=None, help="Claude Code settings file for with-bobbin.")
@click.option("--model", default="claude-opus-4-6", help="Claude model to use.")
@click.option("--budget", default=100.00, type=float, help="Max budget per run (USD).")
@click.option("--timeout", default=3600, type=int, help="Agent timeout in seconds.")
@click.option("--index-timeout", default=600, type=int, help="Bobbin index timeout in seconds.")
@click.option("--skip-verify", is_flag=True, help="Skip test verification at parent commit.")
def run_all(
    tasks_dir: str,
    results_dir: str,
    attempts: int,
    approaches: str,
    settings_file: str | None,
    model: str,
    budget: float,
    timeout: int,
    index_timeout: int,
    skip_verify: bool,
):
    """Run evaluation for all tasks in the tasks directory."""
    tasks_path = _resolve_tasks_dir(tasks_dir)
    rdir = Path(results_dir)

    # Auto-resolve settings file for with-bobbin runs.
    if settings_file is None and _DEFAULT_SETTINGS.exists():
        settings_file = str(_DEFAULT_SETTINGS)

    try:
        tasks = load_all_tasks(tasks_path)
    except TaskLoadError as exc:
        click.echo(f"Error: {exc}", err=True)
        sys.exit(1)

    approach_list = _resolve_approaches(approaches)
    total = len(tasks) * len(approach_list) * attempts
    run_id = _generate_run_id()
    started_at = datetime.now(timezone.utc).isoformat()
    click.echo(
        f"Running {len(tasks)} tasks: {total} total runs "
        f"({len(approach_list)} approaches × {attempts} attempts) "
        f"[run {run_id}]"
    )

    all_results = []
    for task in tasks:
        click.echo(f"\n--- {task['id']}: {task['repo']} ---")
        for approach in approach_list:
            for attempt in range(attempts):
                result = _run_single(
                    task,
                    approach,
                    attempt,
                    rdir,
                    run_id=run_id,
                    settings_file=settings_file,
                    model=model,
                    budget=budget,
                    timeout=timeout,
                    index_timeout=index_timeout,
                    skip_verify=skip_verify,
                )
                all_results.append(result)

    completed_at = datetime.now(timezone.utc).isoformat()
    completed_count = sum(1 for r in all_results if r.get("status") == "completed")
    task_ids = [t["id"] for t in tasks]
    _write_manifest(
        rdir,
        run_id,
        started_at=started_at,
        completed_at=completed_at,
        model=model,
        budget=budget,
        timeout=timeout,
        index_timeout=index_timeout,
        attempts_per_approach=attempts,
        approaches=approach_list,
        tasks=task_ids,
        total_results=len(all_results),
        completed_results=completed_count,
    )

    passed = sum(1 for r in all_results if r.get("test_result", {}).get("passed"))
    click.echo(f"\nAll done: {passed}/{len(all_results)} runs passed tests [run {run_id}]")


@cli.command()
@click.argument("results_dir", default="results")
def score(results_dir: str):
    """Display a summary of existing results.

    RESULTS_DIR is the directory containing result JSON files.
    """
    rdir = Path(results_dir)
    if not rdir.is_dir():
        click.echo(f"Error: Results directory not found: {rdir}", err=True)
        sys.exit(1)

    # Load from run-based and legacy layouts.
    results: list[dict] = []
    runs_dir = rdir / "runs"
    if runs_dir.is_dir():
        for f in sorted(runs_dir.glob("*/*.json")):
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and "task_id" in data:
                    results.append(data)
            except (json.JSONDecodeError, OSError) as exc:
                click.echo(f"Warning: skipping {f.name}: {exc}", err=True)
    for f in sorted(rdir.glob("*.json")):
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            if isinstance(data, dict) and "task_id" in data:
                results.append(data)
        except (json.JSONDecodeError, OSError) as exc:
            click.echo(f"Warning: skipping {f.name}: {exc}", err=True)

    if not results:
        click.echo(f"No result files found in {rdir}", err=True)
        sys.exit(1)

    # Group by approach and compute stats.
    by_approach: dict[str, list[dict]] = {}
    for r in results:
        a = r.get("approach", "unknown")
        by_approach.setdefault(a, []).append(r)

    click.echo(f"\n{'Approach':<16} {'Runs':>5} {'Pass':>5} {'Rate':>8} "
               f"{'Prec':>8} {'Recall':>8} {'F1':>8} {'Avg Time':>10}")
    click.echo("-" * 80)

    for approach in sorted(by_approach):
        runs = by_approach[approach]
        n = len(runs)
        passed = sum(1 for r in runs if r.get("test_result", {}).get("passed"))
        rate = passed / n if n else 0

        precisions = [
            r["diff_result"]["file_precision"]
            for r in runs
            if r.get("diff_result", {}).get("file_precision") is not None
        ]
        recalls = [
            r["diff_result"]["file_recall"]
            for r in runs
            if r.get("diff_result", {}).get("file_recall") is not None
        ]
        f1s = [
            r["diff_result"]["f1"]
            for r in runs
            if r.get("diff_result", {}).get("f1") is not None
        ]
        durations = [
            r["agent_result"]["duration_seconds"]
            for r in runs
            if r.get("agent_result", {}).get("duration_seconds") is not None
        ]

        avg_prec = sum(precisions) / len(precisions) if precisions else 0
        avg_recall = sum(recalls) / len(recalls) if recalls else 0
        avg_f1 = sum(f1s) / len(f1s) if f1s else 0
        avg_time = sum(durations) / len(durations) if durations else 0

        click.echo(
            f"{approach:<16} {n:>5} {passed:>5} {rate:>7.1%} "
            f"{avg_prec:>8.3f} {avg_recall:>8.3f} {avg_f1:>8.3f} {avg_time:>9.1f}s"
        )


@cli.command()
@click.argument("results_dir", default="results")
@click.option("--output", "-o", default=None, help="Output path for the markdown report.")
def report(results_dir: str, output: str | None):
    """Generate a markdown summary report from results.

    RESULTS_DIR is the directory containing result JSON files.
    """
    from analysis.report import ReportError, generate_report

    if output is None:
        output = str(Path(results_dir) / "report.md")

    try:
        generate_report(results_dir, output)
    except ReportError as exc:
        click.echo(f"Error: {exc}", err=True)
        sys.exit(1)

    click.echo(f"Report written to {output}")


def _load_results(results_dir: Path) -> list[dict]:
    """Load all JSON result files from the results directory.

    Scans ``results/runs/*/*.json`` first (run-based layout), then falls
    back to ``results/*.json`` (legacy flat layout).  Manifest and judge
    files are skipped via the ``task_id`` check.
    """
    results: list[dict] = []
    seen_paths: set[Path] = set()

    # Run-based layout: results/runs/<run_id>/<task>_<approach>_<attempt>.json
    runs_dir = results_dir / "runs"
    if runs_dir.is_dir():
        for f in sorted(runs_dir.glob("*/*.json")):
            seen_paths.add(f.resolve())
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, dict) and data.get("status") == "completed":
                    results.append(data)
            except (json.JSONDecodeError, OSError):
                pass

    # Legacy flat layout: results/*.json
    for f in sorted(results_dir.glob("*.json")):
        if f.resolve() in seen_paths:
            continue
        try:
            data = json.loads(f.read_text(encoding="utf-8"))
            if isinstance(data, dict) and data.get("status") == "completed":
                results.append(data)
        except (json.JSONDecodeError, OSError):
            pass

    return results


def _group_results_by_task(results: list[dict]) -> dict[str, dict[str, list[dict]]]:
    """Group results by task_id then by approach.

    Returns ``{task_id: {approach: [results]}}``.
    """
    grouped: dict[str, dict[str, list[dict]]] = {}
    for r in results:
        tid = r.get("task_id", "unknown")
        approach = r.get("approach", "unknown")
        grouped.setdefault(tid, {}).setdefault(approach, []).append(r)
    return grouped


def _pick_best_attempt(runs: list[dict]) -> dict | None:
    """Pick the best attempt from a list of runs for a single task×approach.

    Prefers passing runs, then highest F1, then first attempt.
    """
    if not runs:
        return None
    passing = [r for r in runs if r.get("test_result", {}).get("passed")]
    pool = passing if passing else runs
    return max(pool, key=lambda r: r.get("diff_result", {}).get("f1", 0.0))


@cli.command()
@click.argument("results_dir", default="results")
@click.option(
    "--judge-model",
    default="claude-sonnet-4-5-20250929",
    help="Model to use as the LLM judge.",
)
@click.option(
    "--pairs",
    default="all",
    type=click.Choice(["all", "ai-vs-ai", "human-vs-ai", "human-vs-bobbin"]),
    help="Which pairs to judge.",
)
@click.option("--run", "run_id", default=None, help="Run ID to load/save results from.")
def judge(results_dir: str, judge_model: str, pairs: str, run_id: str | None):
    """Run LLM-as-judge pairwise comparison on stored results.

    Compares three pairs per task:
      - human (ground truth) vs AI (no-bobbin)
      - human (ground truth) vs AI+bobbin (with-bobbin)
      - AI vs AI+bobbin

    Requires that results contain stored diffs (agent_diff + ground_truth_diff).
    Judge results are saved alongside the original results.
    """
    from scorer.llm_judge import LLMJudgeError, judge_pairwise

    rdir = Path(results_dir)
    if not rdir.is_dir():
        click.echo(f"Error: Results directory not found: {rdir}", err=True)
        sys.exit(1)

    # When --run is specified, scope loading and saving to that run directory.
    if run_id:
        run_dir = rdir / "runs" / run_id
        if not run_dir.is_dir():
            click.echo(f"Error: Run directory not found: {run_dir}", err=True)
            sys.exit(1)
        results = _load_results(run_dir)
    else:
        results = _load_results(rdir)

    if not results:
        click.echo(f"Error: No completed results in {rdir}", err=True)
        sys.exit(1)

    # Check that results have stored diffs.
    has_diffs = any("agent_diff" in r for r in results)
    if not has_diffs:
        click.echo(
            "Error: Results do not contain stored diffs (agent_diff). "
            "Re-run evaluations with the updated runner to capture diffs.",
            err=True,
        )
        sys.exit(1)

    grouped = _group_results_by_task(results)
    all_judgements: list[dict] = []

    for task_id, by_approach in sorted(grouped.items()):
        click.echo(f"\n--- Judging {task_id} ---")

        no_bobbin = _pick_best_attempt(by_approach.get("no-bobbin", []))
        with_bobbin = _pick_best_attempt(by_approach.get("with-bobbin", []))

        if not no_bobbin and not with_bobbin:
            click.echo(f"  Skipping {task_id}: no completed runs")
            continue

        # Build context for the judge.
        sample = no_bobbin or with_bobbin
        task_info = sample.get("task", {})
        context = {
            "repo": task_info.get("repo", ""),
            "description": task_info.get("description", task_id),
            "language": task_info.get("language", ""),
        }

        gt_diff = (sample or {}).get("ground_truth_diff", "")

        # Define pairs to judge.
        pair_configs = []
        if pairs in ("all", "ai-vs-ai") and no_bobbin and with_bobbin:
            pair_configs.append({
                "name": "AI vs AI+bobbin",
                "label": "ai-vs-ai+bobbin",
                "diff_a": no_bobbin.get("agent_diff", ""),
                "diff_b": with_bobbin.get("agent_diff", ""),
                "a_label": "no-bobbin",
                "b_label": "with-bobbin",
            })
        if pairs in ("all", "human-vs-ai") and no_bobbin and gt_diff:
            pair_configs.append({
                "name": "Human vs AI",
                "label": "human-vs-ai",
                "diff_a": gt_diff,
                "diff_b": no_bobbin.get("agent_diff", ""),
                "a_label": "human",
                "b_label": "no-bobbin",
            })
        if pairs in ("all", "human-vs-bobbin") and with_bobbin and gt_diff:
            pair_configs.append({
                "name": "Human vs AI+bobbin",
                "label": "human-vs-ai+bobbin",
                "diff_a": gt_diff,
                "diff_b": with_bobbin.get("agent_diff", ""),
                "a_label": "human",
                "b_label": "with-bobbin",
            })

        for pair in pair_configs:
            if not pair["diff_a"].strip() or not pair["diff_b"].strip():
                click.echo(f"  Skipping {pair['name']}: empty diff(s)")
                continue

            click.echo(f"  Judging: {pair['name']}...")
            try:
                verdict = judge_pairwise(
                    pair["diff_a"],
                    pair["diff_b"],
                    context,
                    model=judge_model,
                )
            except LLMJudgeError as exc:
                click.echo(f"    Judge error: {exc}", err=True)
                continue

            # Map winner labels back to meaningful names.
            winner_map = {"a": pair["a_label"], "b": pair["b_label"], "tie": "tie"}
            named_winner = winner_map.get(verdict["overall_winner"], verdict["overall_winner"])

            judgement = {
                "task_id": task_id,
                "pair": pair["label"],
                "a_label": pair["a_label"],
                "b_label": pair["b_label"],
                "overall_winner": verdict["overall_winner"],
                "named_winner": named_winner,
                "dimensions": verdict["dimensions"],
                "bias_detected": verdict.get("bias_detected", False),
                "reasoning": verdict.get("reasoning", ""),
                "judge_model": judge_model,
                "timestamp": datetime.now(timezone.utc).isoformat(),
            }
            all_judgements.append(judgement)

            click.echo(
                f"    Winner: {named_winner}"
                f" (bias={'yes' if verdict.get('bias_detected') else 'no'})"
            )

    # Save all judgements.
    if all_judgements:
        save_dir = rdir / "runs" / run_id if run_id else rdir
        save_dir.mkdir(parents=True, exist_ok=True)
        judge_file = save_dir / "judge_results.json"
        judge_file.write_text(
            json.dumps(all_judgements, indent=2, default=str),
            encoding="utf-8",
        )
        click.echo(f"\nJudge results saved to {judge_file}")
        click.echo(f"Total judgements: {len(all_judgements)}")

        # Print summary.
        click.echo("\nJudge Summary:")
        for pair_label in ("ai-vs-ai+bobbin", "human-vs-ai", "human-vs-ai+bobbin"):
            pair_results = [j for j in all_judgements if j["pair"] == pair_label]
            if not pair_results:
                continue
            wins: dict[str, int] = {}
            for j in pair_results:
                w = j["named_winner"]
                wins[w] = wins.get(w, 0) + 1
            total = len(pair_results)
            parts = [f"{name}: {count}/{total}" for name, count in sorted(wins.items())]
            click.echo(f"  {pair_label}: {', '.join(parts)}")
    else:
        click.echo("\nNo judgements were produced.")


@cli.command()
@click.argument("results_dir", default="results")
@click.option(
    "--output-dir",
    default=None,
    help="Output directory for generated pages (default: docs/book/src/eval).",
)
@click.option("--tasks-dir", default="tasks", help="Directory containing task YAML files.")
@click.option("--run", "run_id", default=None, help="Publish a specific run (default: latest).")
@click.option("--all-runs", is_flag=True, help="Include historical trends from all runs.")
def publish(results_dir: str, output_dir: str | None, tasks_dir: str, run_id: str | None, all_runs: bool):
    """Generate mdbook pages from eval results.

    Reads results JSON files and task definitions, then generates markdown
    pages with matplotlib SVG charts for the documentation site.
    """
    from analysis.mdbook_pages import PageGenError, generate_all_pages

    if output_dir is None:
        output_dir = str(_EVAL_ROOT.parent / "docs" / "book" / "src" / "eval")

    tasks_path = _resolve_tasks_dir(tasks_dir)

    try:
        generated = generate_all_pages(
            results_dir,
            str(tasks_path),
            output_dir,
            run_id=run_id,
            include_trends=all_runs,
        )
    except PageGenError as exc:
        click.echo(f"Error: {exc}", err=True)
        sys.exit(1)

    click.echo(f"Generated {len(generated)} pages in {output_dir}:")
    for f in generated:
        click.echo(f"  {f}")


if __name__ == "__main__":
    cli()
