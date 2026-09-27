"""Run every untouched pilot task's free parent/fix controls; never invoke an agent."""
from __future__ import annotations

import argparse
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
    parser.add_argument("source", type=Path)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    controls(args.source, args.manifest_sha256, args.output,
             Path(__file__).resolve().parent.parent)


if __name__ == "__main__":
    main()
