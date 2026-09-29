"""Run every untouched pilot task's free parent/fix controls; never invoke an agent."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from runner import pilot
from runner.pilot_resume import plan
from runner.task_loader import load_task_by_id


def controls(source, manifest_sha256, output, root):
    if output.resolve().is_relative_to(source.resolve()):
        raise ValueError("fixture output must be outside the preserved campaign")
    recovery = plan(source, root, manifest_sha256)
    tasks = list(dict.fromkeys(task for task, _ in recovery["remaining_schedule"]))
    manifest = json.loads((source / "manifest.json").read_text())
    timeout = manifest.get("timeout")
    if not isinstance(timeout, int) or not 0 < timeout <= 3600:
        raise ValueError("original timeout must be between 1 and 3600 seconds")
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    pilot.write_json(output / "recovery-plan.json", recovery)
    report = {"kind": "fixture-controls-only", "model_calls": 0,
              "source_manifest_sha256": manifest_sha256, "tasks": [],
              "all_tasks_checked": False}
    return _run(tasks, timeout, output, root, report)


def standalone(task_ids, timeout, output, root):
    """Run the free controls over an EXPLICIT task list with no campaign binding.

    `controls` refuses any task whose definition changed since the original
    campaign, which is right for continuing paid cells and makes it impossible
    to measure REPAIRED fixtures (aegis-bgk9ho). This mode takes the ids from
    the operator and records the sha256 of every task file it used, so the
    report says exactly which definitions it measured. It never touches a
    campaign and makes no model calls.
    """
    if not task_ids or len(set(task_ids)) != len(task_ids):
        raise ValueError("standalone controls need a non-empty list of distinct task ids")
    if not isinstance(timeout, int) or not 0 < timeout <= 3600:
        raise ValueError("timeout must be between 1 and 3600 seconds")
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    digests = {}
    for task_id in task_ids:
        path = root / "tasks" / f"{task_id}.yaml"
        digests[task_id] = hashlib.sha256(path.read_bytes()).hexdigest()
    report = {"kind": "fixture-controls-standalone", "model_calls": 0,
              "timeout": timeout, "task_sha256": digests, "tasks": [],
              "all_tasks_checked": False}
    return _run(list(task_ids), timeout, output, root, report)


def _run(tasks, timeout, output, root, report):
    pilot.write_json(output / "report.json", report)
    for task_id in tasks:
        task_output = output / task_id
        task_output.mkdir()
        with tempfile.TemporaryDirectory(prefix="fixture-", dir=output) as temp:
            scratch = Path(temp)
            env = pilot.isolated_env(scratch / "home", copy_credentials=False)
            # This stage needs no model credentials even when inherited by
            # the parent evaluation shell. It does not look up either CLI.
            for key in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY"):
                env.pop(key, None)
            row = {"task": task_id, "eligible": False}
            try:
                task = load_task_by_id(task_id, root / "tasks")
                pilot.prepare(task, scratch, env, timeout)
                row.update(eligible=True, status="fixture_pass")
            except ValueError as exc:
                row.update(status="fixture_exclusion", reason=str(exc))
            except (OSError, subprocess.SubprocessError) as exc:
                # Infrastructure trouble is a recorded exclusion, never an
                # arm score or a reason to lose the other tasks' controls.
                row.update(status="infrastructure_exclusion", reason=type(exc).__name__)
            finally:
                receipt = scratch / "controls.json"
                if receipt.exists():
                    shutil.copy2(receipt, task_output / "controls.json")
            report["tasks"].append(row)
            pilot.write_json(output / "report.json", report)
            print(f"{task_id}: {row['status']}", flush=True)
    report["all_tasks_checked"] = len(report["tasks"]) == len(tasks)
    report["eligible_tasks"] = [r["task"] for r in report["tasks"] if r["eligible"]]
    pilot.write_json(output / "report.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, nargs="?")
    parser.add_argument("--manifest-sha256")
    parser.add_argument("--tasks", help="comma-separated ids: standalone mode, no campaign")
    parser.add_argument("--timeout", type=int, default=900,
                        help="per-command seconds in standalone mode")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    root = Path(__file__).resolve().parent.parent
    if args.tasks:
        if args.source or args.manifest_sha256:
            parser.error("--tasks is standalone: do not pass a campaign source or manifest")
        standalone([t for t in args.tasks.split(",") if t], args.timeout, args.output, root)
    else:
        if not (args.source and args.manifest_sha256):
            parser.error("campaign mode needs SOURCE and --manifest-sha256")
        controls(args.source, args.manifest_sha256, args.output, root)


if __name__ == "__main__":
    main()
