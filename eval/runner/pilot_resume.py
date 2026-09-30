"""Conservative, read-only planning for an interrupted paid pilot.

The plan excludes whole attempted tasks, including incomplete pairs. It does
not authorize spending, retry failed fixtures, or turn missing cells into scores.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected a regular pinned file: {path.name}")
    return path


def original_file(root, commit, name):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("invalid original harness commit")
    try:
        return subprocess.check_output(
            ["git", "show", f"{commit}:eval/{name}"], cwd=root,
            stderr=subprocess.PIPE, timeout=20,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
        raise ValueError(f"original source unavailable: {name}") from exc


def plan(source, root, expected_manifest, allow_amended=False):
    """Validate the original campaign and classify every preregistered slot.

    No imports of the agent runner, credential access, writes or model calls.
    The operator supplies the exact manifest digest reviewed for continuation.
    ``allow_amended`` (chained continuations only) records a changed task
    definition instead of refusing it, provided the task has no paid evidence.
    """
    if source.is_symlink() or not source.is_dir():
        raise ValueError("source must be a real campaign directory")
    manifest_path = regular(source / "manifest.json")
    if digest(manifest_path) != expected_manifest:
        raise ValueError("original manifest digest mismatch")
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("kind") != "pilot" or "continuation" in manifest:
        raise ValueError("only an original pilot is supported; chained resumes need review")
    commit = manifest["harness_commit"]
    prereg = original_file(root, commit, "v2/PREREGISTRATION.md")
    if hashlib.sha256(prereg).hexdigest() != manifest["preregistration_sha256"]:
        raise ValueError("original preregistration digest mismatch")
    raw_tasks = original_file(root, commit, "v2/pilot-tasks.txt").decode()
    tasks = [t.strip() for t in raw_tasks.splitlines() if t.strip() and not t.startswith("#")]
    if len(tasks) != 30 or len(set(tasks)) != 30:
        raise ValueError("original pilot must contain 30 distinct tasks")
    if any(not re.fullmatch(r"[a-zA-Z0-9_-]+", task) for task in tasks):
        raise ValueError("unsafe task id")
    order = manifest.get("schedule")
    expected = {(task, cell) for task in tasks for cell in ("none", "tool", "inject", "both")}
    try:
        pairs = [tuple(pair) for pair in order]
        valid = len(pairs) == 120 and set(pairs) == expected
    except (TypeError, ValueError):
        valid = False
    if not valid:
        raise ValueError("original schedule is not the complete paired pilot")
    # A source with a task not in the manifest is not silently ignored.
    allowed = set(tasks) | {"bin", "repos", "manifest.json", "runtime-verification.json",
                            "continuation-claim.json"}
    if {p.name for p in source.iterdir()} - allowed:
        raise ValueError("unrecognized campaign artifacts; review before continuation")
    binary = source / "bin"
    if binary.is_symlink() or not binary.is_dir():
        raise ValueError("invalid pinned binary directory")
    runtime = manifest.get("embedding_runtime")
    pins = {"bobbin.real" if runtime else "bobbin": manifest["bobbin_sha256"]}
    if runtime:
        pins["bobbin"] = manifest["gpu_guard_sha256"]
        if manifest.get("runtime_verifier_sha256"):
            pins["runtime_lock.py"] = manifest["runtime_verifier_sha256"]
        if json.loads(regular(binary / "gpu-runtime.json").read_text()) != runtime:
            raise ValueError("pinned GPU runtime configuration differs from manifest")
    for name, expected_hash in pins.items():
        if digest(regular(binary / name)) != expected_hash:
            raise ValueError(f"pinned executable digest mismatch: {name}")
    artifacts, task_hashes, excluded, amended = {}, {}, [], {}
    for task in tasks:
        original = original_file(root, commit, f"tasks/{task}.yaml")
        current = regular(root / "tasks" / f"{task}.yaml").read_bytes()
        if original != current:
            if not allow_amended:
                raise ValueError(f"task definition changed: {task}")
            amended[task] = {"original": hashlib.sha256(original).hexdigest(),
                             "current": hashlib.sha256(current).hexdigest()}
        task_hashes[task] = hashlib.sha256(original).hexdigest()
        directory = source / task
        if not directory.exists() and not directory.is_symlink():
            continue
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"invalid attempted task directory: {task}")
        results = streams = 0
        # Any directory is an attempt, even empty or killed before result.json.
        for path in directory.rglob("*"):
            if path.is_symlink():
                raise ValueError(f"symlink in prior evidence: {task}")
            if path.is_file():
                artifacts[str(path.relative_to(source))] = digest(path)
                results += path.name == "result.json"
                streams += path.name == "stream.jsonl"
        if task in amended and (results or streams):
            raise ValueError(f"task definition changed after paid evidence: {task}")
        excluded.append({"task": task, "reason": "previously_attempted",
                         "result_records": results, "stream_records": streams,
                         "fixture_error": (directory / "fixture-error.json").is_file()})
    skipped = {item["task"] for item in excluded}
    extra = {"amended_task_sha256": amended} if allow_amended else {}
    return {**extra, "source": str(source.resolve()), "source_manifest_sha256": expected_manifest,
            "source_harness_commit": commit, "source_pins": pins,
            "original_parameters": {key: manifest.get(key) for key in (
                "model", "budget_per_run", "timeout", "max_turns",
                "preregistration_sha256")},
            "runtime_library_lock_present": bool(runtime and runtime.get("library_lock_version")),
            "task_sha256": task_hashes, "prior_artifact_sha256": artifacts,
            "excluded_tasks": excluded,
            "remaining_schedule": [list(pair) for pair in pairs if pair[0] not in skipped],
            "spend_authorized": False, "partial_pairs_are_complete": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--manifest-sha256", required=True)
    args = parser.parse_args()
    try:
        result = plan(args.source, Path(__file__).resolve().parent.parent,
                      args.manifest_sha256)
    except (OSError, ValueError, KeyError) as exc:
        parser.exit(2, f"resume plan refused: {exc}\n")
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
