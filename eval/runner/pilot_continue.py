"""Reviewed single continuation of a pilot; default mode only prints its plan."""
from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import subprocess
from pathlib import Path

from runner import pilot
from runner.pilot_resume import digest, plan, regular
from runner.runtime_lock import verify_runtime


def pinned_json(path, sha):
    if digest(regular(path)) != sha:
        raise ValueError(f"reviewed input digest mismatch: {path.name}")
    return json.loads(path.read_text())


def build_plan(source, manifest_sha, report_path, report_sha, lock_path, lock_sha,
               claude, claude_sha, root):
    recovery = plan(source, root, manifest_sha)
    if (source / "continuation-claim.json").exists():
        raise ValueError("original campaign already has a continuation claim")
    manifest = pinned_json(source / "manifest.json", manifest_sha)
    if manifest.get("model") != pilot.MODEL:
        raise ValueError("original model differs from the current pilot model")
    for key in ("budget_per_run", "timeout", "max_turns"):
        value = manifest.get(key)
        if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
            raise ValueError(f"invalid original parameter: {key}")
    report = pinned_json(report_path, report_sha)
    if (report.get("kind") != "fixture-controls-only"
            or report.get("all_tasks_checked") is not True
            or type(report.get("model_calls")) is not int or report["model_calls"] != 0
            or report.get("source_manifest_sha256") != manifest_sha):
        raise ValueError("complete matching no-spend fixture report required")
    recorded_plan = json.loads(regular(report_path.parent / "recovery-plan.json").read_text())
    if recorded_plan != recovery:
        raise ValueError("fixture inputs or original evidence changed since preflight")
    tasks = list(dict.fromkeys(task for task, _ in recovery["remaining_schedule"]))
    rows = report.get("tasks", [])
    if [row.get("task") for row in rows] != tasks:
        raise ValueError("fixture report must cover every untouched task exactly once in order")
    eligible, control_pins = [], {}
    for row in rows:
        if row.get("eligible") is not True:
            if row.get("eligible") is not False or row.get("status") not in (
                    "fixture_exclusion", "infrastructure_exclusion"):
                raise ValueError("invalid fixture exclusion")
            continue
        if row.get("status") != "fixture_pass":
            raise ValueError("eligible task lacks passing fixture status")
        receipt = regular(report_path.parent / row["task"] / "controls.json")
        controls = json.loads(receipt.read_text())
        negative, positive = controls.get("parent", {}), controls.get("fix", {})
        if not (negative.get("valid") is True and negative.get("passed") is False
                and positive.get("valid") is True and positive.get("passed") is True
                and negative.get("executed", 0) > 0 and positive.get("executed", 0) > 0):
            raise ValueError("fixture receipt does not discriminate parent from fix")
        eligible.append(row["task"])
        control_pins[str(receipt.absolute())] = digest(receipt)
    if report.get("eligible_tasks") != eligible:
        raise ValueError("eligible task list disagrees with control rows")
    runtime = pinned_json(lock_path, lock_sha)
    original_runtime = manifest.get("embedding_runtime")
    if not isinstance(original_runtime, dict) or not original_runtime.get("artifacts"):
        raise ValueError("continuation requires the original pinned GPU archive configuration")
    if any(runtime.get(key) != value for key, value in original_runtime.items()):
        raise ValueError("runtime lock changed original archive/configuration pins")
    if digest(regular(claude)) != claude_sha:
        raise ValueError("reviewed client executable changed")
    verification = verify_runtime(runtime)
    return {"recovery": recovery, "fixture_report_sha256": report_sha,
            "fixture_report_path": str(report_path.resolve()),
            "fixture_control_sha256": control_pins, "runtime_lock_sha256": lock_sha,
            "runtime_verified_libraries": verification["verified_libraries"],
            "client_sha256": claude_sha, "client_path": str(claude.absolute()),
            "original_client_was_pinned": False,
            "schedule": [pair for pair in recovery["remaining_schedule"] if pair[0] in eligible],
            "fixture_exclusions": [row for row in rows if row["eligible"] is False]}


def execute(execution, source, output, lock_path, root, revalidate, claim_dir=None,
            evidence=None):
    """Claim once, then run the eligible schedule.

    ``claim_dir`` holds the single-use claim (default: the original campaign).
    ``evidence`` re-checks prior evidence before every paid cell; by default it
    re-plans the original campaign. A chained continuation supplies both.
    """
    output = output.absolute()
    claim_dir = source if claim_dir is None else claim_dir
    if (output.exists() or output.resolve().is_relative_to(source.resolve())
            or output.resolve().is_relative_to(claim_dir.resolve())):
        raise ValueError("continuation output must be new and outside the original campaign")
    if evidence is None:
        def evidence():
            current = plan(source, root, execution["recovery"]["source_manifest_sha256"])
            return current == execution["recovery"]
    if not execution["schedule"]:
        raise ValueError("no eligible untouched tasks; no continuation will be claimed")
    # Validate everything again before the single-use claim. A failed setup or
    # interrupted run leaves the claim in place: never automatically retry it.
    if revalidate() != execution:
        raise ValueError("continuation inputs changed before claim")
    runtime = pinned_json(lock_path, execution["runtime_lock_sha256"])
    claim = claim_dir / "continuation-claim.json"
    fd = os.open(claim, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        json.dump({"output": str(output.resolve()), "execution": execution}, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    output.mkdir(parents=True, mode=0o700)
    original = json.loads((source / "manifest.json").read_text())
    pinned = output / "bin"
    pinned.mkdir()
    shutil.copy2(source / "bin/bobbin.real", pinned / "bobbin.real")
    if digest(pinned / "bobbin.real") != execution["recovery"]["source_pins"]["bobbin.real"]:
        raise RuntimeError("original executable changed during copy; claim retained")
    for name in ("gpu_guard.py", "runtime_lock.py"):
        shutil.copy2(root / "runner" / name, pinned / ("bobbin" if name == "gpu_guard.py" else name))
    (pinned / "bobbin").chmod(0o755)
    pilot.write_json(pinned / "gpu-runtime.json", runtime)
    bin_pins = {name: digest(pinned / name) for name in
                ("bobbin", "bobbin.real", "runtime_lock.py", "gpu-runtime.json")}
    runtime_receipt = pilot.verify_and_record(runtime, output / "runtime-verification.json")
    manifest = {**original, "continuation": execution, "schedule": execution["schedule"],
                "embedding_runtime": runtime,
                "runtime_verification": runtime_receipt,
                "harness_commit": subprocess.check_output(
                    ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                "preregistration_sha256": digest(root / "v2/PREREGISTRATION.md"),
                "gpu_guard_sha256": bin_pins["bobbin"],
                "runtime_verifier_sha256": bin_pins["runtime_lock.py"]}
    pilot.write_json(output / "manifest.json", manifest)
    expected_claim = digest(claim)

    def before_spend():
        # The original single-use claim is the only new source artifact.
        try:
            if not evidence() or digest(claim) != expected_claim:
                raise ValueError("original evidence or continuation claim changed")
            if digest(regular(Path(execution["client_path"]))) != execution["client_sha256"]:
                raise ValueError("client executable changed")
            if any(digest(regular(pinned / name)) != sha for name, sha in bin_pins.items()):
                raise ValueError("continuation executable/configuration changed")
            if any(digest(regular(Path(path))) != sha
                   for path, sha in execution["fixture_control_sha256"].items()):
                raise ValueError("free fixture receipt changed")
            if digest(regular(Path(execution["fixture_report_path"]))) != execution["fixture_report_sha256"]:
                raise ValueError("free fixture report changed")
        except (OSError, ValueError) as exc:
            raise RuntimeError(f"continuation stopped before spending: {exc}") from exc

    for task in dict.fromkeys(task for task, _ in execution["schedule"]):
        before_spend()
        pilot.run_task(task, output, pinned, str(pinned / "bobbin"), execution["client_path"],
                       execution["schedule"], runtime, original["budget_per_run"],
                       original["timeout"], original["max_turns"], before_spend=before_spend)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    for name in ("manifest", "controls", "runtime", "client"):
        if name != "manifest":
            parser.add_argument(f"--{name}", required=True, type=Path)
        parser.add_argument(f"--{name}-sha256", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    def validate():
        return build_plan(args.source, args.manifest_sha256, args.controls, args.controls_sha256,
                          args.runtime, args.runtime_sha256, args.client, args.client_sha256, root)
    execution = validate()
    if not args.execute:
        print(json.dumps(execution, indent=2, sort_keys=True))
        return
    if args.output is None:
        parser.error("--execute requires a new --output directory")
    os.umask(0o077)
    execute(execution, args.source, args.output, args.runtime, root, validate)


if __name__ == "__main__":
    main()
