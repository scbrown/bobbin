"""Reviewed second continuation of a pilot, chained to a finished first one.

Default mode only prints its plan. Every task attempted by the original run or
by the first continuation is skipped, whatever its outcome. The claim is taken
exclusively in the first continuation's output, so this chain can run once.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path

from runner import pilot, pilot_continue
from runner.pilot_resume import digest, plan, regular
from runner.runtime_lock import verify_runtime

# Fields of the original recovery plan that must be unchanged since the first
# continuation was claimed. task_sha256/amended fields legitimately differ.
STABLE = ("source", "source_manifest_sha256", "source_harness_commit", "source_pins",
          "original_parameters", "prior_artifact_sha256", "excluded_tasks",
          "remaining_schedule")
PARENT_FIXED = {"bin", "repos", "manifest.json", "runtime-verification.json"}


def parent_evidence(parent, tasks):
    """Hash every file under the first continuation's task directories."""
    allowed = PARENT_FIXED | set(tasks) | {"continuation-claim.json"}
    if {p.name for p in parent.iterdir()} - allowed:
        raise ValueError("unrecognized first-continuation artifacts; review before chaining")
    artifacts, attempted = {}, []
    for task in tasks:
        directory = parent / task
        if not directory.exists() and not directory.is_symlink():
            continue
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"invalid first-continuation task directory: {task}")
        for path in directory.rglob("*"):
            if path.is_symlink():
                raise ValueError(f"symlink in first-continuation evidence: {task}")
            if path.is_file():
                artifacts[str(path.relative_to(parent))] = digest(path)
        attempted.append(task)
    return artifacts, attempted


def evidence_digest(artifacts):
    """One reviewable digest over the first continuation's per-file hashes."""
    return hashlib.sha256(json.dumps(artifacts, sort_keys=True).encode()).hexdigest()


def build_plan(source, manifest_sha, parent, parent_sha, evidence_sha, report_path,
               report_sha, lock_path, lock_sha, claude, claude_sha, root,
               allow_parent_claim=False):
    recovery = plan(source, root, manifest_sha, allow_amended=True)
    if parent.is_symlink() or not parent.is_dir():
        raise ValueError("first continuation must be a real directory")
    if (parent / "continuation-claim.json").exists() and not allow_parent_claim:
        raise ValueError("first continuation already has a chained claim")
    claim = json.loads(regular(source / "continuation-claim.json").read_text())
    parent_manifest = pilot_continue.pinned_json(parent / "manifest.json", parent_sha)
    first = parent_manifest.get("continuation")
    if (claim.get("output") != str(parent.resolve()) or not isinstance(first, dict)
            or claim.get("execution") != first):
        raise ValueError("first continuation is not the one claimed by the original campaign")
    if "parent" in first:
        raise ValueError("only a single chained continuation is supported; review deeper chains")
    if any(first["recovery"].get(key) != recovery.get(key) for key in STABLE):
        raise ValueError("original evidence changed since the first continuation")
    if lock_sha != first.get("runtime_lock_sha256") or claude_sha != first.get("client_sha256"):
        raise ValueError("chained continuation must reuse the first continuation's runtime and client pins")
    parent_tasks = list(dict.fromkeys(task for task, _ in first["schedule"]))
    if set(parent_tasks) & set(recovery["amended_task_sha256"]):
        raise ValueError("task definition changed after the first continuation ran it")
    parent_artifacts, _ = parent_evidence(parent, parent_tasks)
    if evidence_digest(parent_artifacts) != evidence_sha:
        raise ValueError("first-continuation evidence differs from the reviewed digest")
    remaining = [pair for pair in recovery["remaining_schedule"] if pair[0] not in parent_tasks]
    tasks = list(dict.fromkeys(task for task, _ in remaining))
    report = pilot_continue.pinned_json(report_path, report_sha)
    if (report.get("kind") != "fixture-controls-standalone"
            or report.get("all_tasks_checked") is not True
            or type(report.get("model_calls")) is not int or report["model_calls"] != 0):
        raise ValueError("complete no-spend standalone fixture report required")
    rows = report.get("tasks", [])
    if [row.get("task") for row in rows] != tasks:
        raise ValueError("fixture report must cover every never-attempted task exactly once in order")
    current = {task: digest(regular(root / "tasks" / f"{task}.yaml")) for task in tasks}
    if report.get("task_sha256") != current:
        raise ValueError("fixture report measured different task definitions than will run")
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
    runtime = pilot_continue.pinned_json(lock_path, lock_sha)
    original_runtime = pilot_continue.pinned_json(source / "manifest.json", manifest_sha).get(
        "embedding_runtime")
    if not isinstance(original_runtime, dict) or not original_runtime.get("artifacts"):
        raise ValueError("continuation requires the original pinned GPU archive configuration")
    if any(runtime.get(key) != value for key, value in original_runtime.items()):
        raise ValueError("runtime lock changed original archive/configuration pins")
    if digest(regular(claude)) != claude_sha:
        raise ValueError("reviewed client executable changed")
    verification = verify_runtime(runtime)
    schedule = [pair for pair in remaining if pair[0] in eligible]
    budget = recovery["original_parameters"]["budget_per_run"]
    return {"recovery": recovery,
            "max_spend_usd": {"cells": len(schedule), "budget_per_cell": budget,
                              "total": round(len(schedule) * budget, 2)}, "fixture_report_sha256": report_sha,
            "fixture_report_path": str(report_path.resolve()),
            "fixture_control_sha256": control_pins, "runtime_lock_sha256": lock_sha,
            "runtime_verified_libraries": verification["verified_libraries"],
            "client_sha256": claude_sha, "client_path": str(claude.absolute()),
            "original_client_was_pinned": False,
            "parent": {"path": str(parent.resolve()), "manifest_sha256": parent_sha,
                       "tasks": parent_tasks, "artifact_sha256": parent_artifacts,
                       "evidence_sha256": evidence_sha},
            "task_sha256": current,
            "schedule": schedule,
            "fixture_exclusions": [row for row in rows if row["eligible"] is False]}


CELLS = ("none", "tool", "inject", "both")
# Files a task directory can hold before its first cell directory exists.
PRE_CELL = {"controls.json", "index.json", "fixture-error.json", "infrastructure-error.json"}


def chain_evidence(output, tasks):
    """Hash a stopped chain's task evidence and split spent from unspent tasks.

    A cell directory is created before the spend check and the agent call, so
    a task directory without one never reached a model. Any cell directory,
    stream or result makes the whole task spent, and it is never re-run.
    """
    allowed = PARENT_FIXED | set(tasks) | {"continuation-claim.json"}
    if {p.name for p in output.iterdir()} - allowed:
        raise ValueError("unrecognized chained-run artifacts; review before resuming")
    artifacts, spent, unspent = {}, [], []
    for task in tasks:
        directory = output / task
        if not directory.exists() and not directory.is_symlink():
            continue
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"invalid chained-run task directory: {task}")
        names = set()
        for path in directory.rglob("*"):
            if path.is_symlink():
                raise ValueError(f"symlink in chained-run evidence: {task}")
            if path.is_file():
                artifacts[str(path.relative_to(output))] = digest(path)
            if path.parent == directory:
                names.add(path.name)
        if names - PRE_CELL:
            spent.append(task)
        else:
            unspent.append(task)
    return artifacts, spent, unspent


def build_resume_plan(chain, chain_sha, source, manifest_sha, parent, parent_sha,
                      evidence_sha, report_path, report_sha, lock_path, lock_sha,
                      claude, claude_sha, root):
    """Plan the single resume of a stopped chained continuation.

    The recorded execution must be reproduced exactly from the same reviewed
    inputs; only spent tasks leave the schedule. Nothing is claimed here.
    """
    if chain.is_symlink() or not chain.is_dir():
        raise ValueError("stopped chained run must be a real directory")
    if (chain / "continuation-claim.json").exists():
        raise ValueError("chained run already has a resume claim")
    manifest = pilot_continue.pinned_json(chain / "manifest.json", chain_sha)
    first = manifest.get("continuation")
    if not isinstance(first, dict) or "parent" not in first or "resume" in first:
        raise ValueError("only a stopped first chained continuation can be resumed")
    claim = json.loads(regular(parent / "continuation-claim.json").read_text())
    if claim.get("output") != str(chain.resolve()) or claim.get("execution") != first:
        raise ValueError("chained run is not the one claimed in the first continuation")
    current = build_plan(source, manifest_sha, parent, parent_sha, evidence_sha, report_path,
                         report_sha, lock_path, lock_sha, claude, claude_sha, root,
                         allow_parent_claim=True)
    if current != first:
        raise ValueError("reviewed inputs no longer reproduce the stopped chained run")
    guard = digest(regular(root / "runner" / "gpu_guard.py"))
    if guard != manifest.get("gpu_guard_sha256"):
        raise ValueError("resume would change the GPU wrapper; review required")
    tasks = list(dict.fromkeys(task for task, _ in first["schedule"]))
    artifacts, spent, unspent = chain_evidence(chain, tasks)
    schedule = [pair for pair in first["schedule"] if pair[0] not in spent]
    if not schedule:
        raise ValueError("no unspent tasks remain; nothing to resume")
    budget = first["recovery"]["original_parameters"]["budget_per_run"]
    return {**first, "schedule": schedule,
            "max_spend_usd": {"cells": len(schedule), "budget_per_cell": budget,
                              "total": round(len(schedule) * budget, 2)},
            "resume": {"path": str(chain.resolve()), "manifest_sha256": chain_sha,
                       "artifact_sha256": artifacts, "spent_tasks": spent,
                       "reattempted_unspent_tasks": unspent,
                       "runtime_verifier_sha256_before": manifest.get("runtime_verifier_sha256"),
                       "runtime_verifier_sha256_after": digest(
                           regular(root / "runner" / "runtime_lock.py"))}}


def execute(execution, source, parent, output, lock_path, root, revalidate):
    resume = execution.get("resume")
    claim_dir = Path(resume["path"]) if resume else parent
    if output.absolute().resolve().is_relative_to(parent.resolve()):
        raise ValueError("continuation output must be new and outside the first continuation")

    def evidence():
        current = plan(source, root, execution["recovery"]["source_manifest_sha256"],
                       allow_amended=True)
        if current != execution["recovery"]:
            return False
        if digest(regular(parent / "manifest.json")) != execution["parent"]["manifest_sha256"]:
            return False
        if parent_evidence(parent, execution["parent"]["tasks"])[0] != execution["parent"]["artifact_sha256"]:
            return False
        if resume:
            chain = Path(resume["path"])
            if digest(regular(chain / "manifest.json")) != resume["manifest_sha256"]:
                return False
            tasks = list(dict.fromkeys(task for task, _ in json.loads(
                (chain / "manifest.json").read_text())["continuation"]["schedule"]))
            if chain_evidence(chain, tasks)[0] != resume["artifact_sha256"]:
                return False
        return all(digest(regular(root / "tasks" / f"{task}.yaml")) == sha
                   for task, sha in execution["task_sha256"].items())
    pilot_continue.execute(execution, source, output, lock_path, root, revalidate,
                           claim_dir=claim_dir, evidence=evidence)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--parent", required=True, type=Path)
    parser.add_argument("--parent-sha256", required=True)
    parser.add_argument("--parent-evidence-sha256", required=True,
                        help="digest printed as parent.evidence_sha256 by a reviewed dry run")
    for name in ("controls", "runtime", "client"):
        parser.add_argument(f"--{name}", required=True, type=Path)
        parser.add_argument(f"--{name}-sha256", required=True)
    parser.add_argument("--resume", type=Path,
                        help="stopped chained-run output to resume once (skips spent tasks)")
    parser.add_argument("--resume-manifest-sha256")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if bool(args.resume) != bool(args.resume_manifest_sha256):
        parser.error("--resume and --resume-manifest-sha256 go together")

    def validate():
        if args.resume:
            return build_resume_plan(
                args.resume, args.resume_manifest_sha256, args.source, args.manifest_sha256,
                args.parent, args.parent_sha256, args.parent_evidence_sha256, args.controls,
                args.controls_sha256, args.runtime, args.runtime_sha256, args.client,
                args.client_sha256, root)
        return build_plan(args.source, args.manifest_sha256, args.parent, args.parent_sha256,
                          args.parent_evidence_sha256, args.controls, args.controls_sha256, args.runtime,
                          args.runtime_sha256, args.client, args.client_sha256, root)
    execution = validate()
    if not args.execute:
        print(json.dumps(execution, indent=2, sort_keys=True))
        return
    if args.output is None:
        parser.error("--execute requires a new --output directory")
    os.umask(0o077)
    execute(execution, args.source, args.parent, args.output, args.runtime, root, validate)


if __name__ == "__main__":
    main()
