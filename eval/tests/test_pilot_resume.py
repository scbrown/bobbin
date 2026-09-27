"""No-spend recovery controls use synthetic campaigns, never paid outputs."""
import hashlib
import json

import pytest

from runner import pilot_resume as resume


@pytest.fixture
def campaign(tmp_path, monkeypatch):
    source, root = tmp_path / "source", tmp_path / "eval"
    (source / "bin").mkdir(parents=True)
    (root / "tasks").mkdir(parents=True)
    tasks = [f"task-{i:03}" for i in range(30)]
    original = {"v2/pilot-tasks.txt": "\n".join(tasks).encode(),
                "v2/PREREGISTRATION.md": b"original protocol"}
    for task in tasks:
        data = f"id: {task}\n".encode()
        original[f"tasks/{task}.yaml"] = data
        (root / "tasks" / f"{task}.yaml").write_bytes(data)
    monkeypatch.setattr(resume, "original_file", lambda r, c, name: original[name])
    (source / "bin/bobbin").write_bytes(b"frozen binary")
    manifest = {"kind": "pilot", "harness_commit": "1" * 40,
                "schedule": [[task, cell] for task in tasks
                             for cell in ("none", "tool", "inject", "both")],
                "preregistration_sha256": hashlib.sha256(original[
                    "v2/PREREGISTRATION.md"]).hexdigest(),
                "bobbin_sha256": resume.digest(source / "bin/bobbin")}
    (source / "manifest.json").write_text(json.dumps(manifest))
    return source, root, resume.digest(source / "manifest.json")


def test_excludes_every_attempt_including_partial_and_empty_without_writes(campaign):
    source, root, sha = campaign
    partial = source / "task-000/none"
    partial.mkdir(parents=True)
    (partial / "stream.jsonl").write_text("already spent; no result saved")
    fixture = source / "task-001"
    fixture.mkdir()
    (fixture / "fixture-error.json").write_text('{}')
    (source / "task-002").mkdir()
    before = {str(p): (p.read_bytes(), p.stat().st_mtime_ns)
              for p in source.rglob("*") if p.is_file()}
    plan = resume.plan(source, root, sha)
    assert len(plan["remaining_schedule"]) == 108
    assert {r["task"] for r in plan["excluded_tasks"]} == {
        "task-000", "task-001", "task-002"}
    assert plan["excluded_tasks"][0]["stream_records"] == 1
    assert plan["excluded_tasks"][0]["result_records"] == 0
    assert plan["spend_authorized"] is False
    assert plan["partial_pairs_are_complete"] is False
    assert before == {str(p): (p.read_bytes(), p.stat().st_mtime_ns)
                      for p in source.rglob("*") if p.is_file()}


@pytest.mark.parametrize("tamper", ["manifest", "binary", "task", "symlink", "unknown"])
def test_refuses_changed_or_ambiguous_input(campaign, tamper):
    source, root, sha = campaign
    if tamper == "manifest":
        (source / "manifest.json").write_text('{}')
    elif tamper == "binary":
        (source / "bin/bobbin").write_bytes(b"replacement")
    elif tamper == "task":
        (root / "tasks/task-000.yaml").write_text('changed')
    elif tamper == "symlink":
        (source / "task-000").symlink_to(root, target_is_directory=True)
    else:
        (source / "unrecognized").mkdir()
    with pytest.raises(ValueError):
        resume.plan(source, root, sha)


@pytest.mark.parametrize("change", ["duplicate", "unsafe", "chained"])
def test_refuses_incompatible_schedule_or_continuation(campaign, change):
    source, root, _ = campaign
    path = source / "manifest.json"
    manifest = json.loads(path.read_text())
    if change == "duplicate":
        manifest["schedule"][0] = manifest["schedule"][1]
    elif change == "unsafe":
        manifest["schedule"][0] = ["../elsewhere", "none"]
    else:
        manifest["continuation"] = {}
    path.write_text(json.dumps(manifest))
    with pytest.raises(ValueError):
        resume.plan(source, root, resume.digest(path))


def test_gpu_wrapper_and_configuration_pins_checked(campaign):
    source, root, _ = campaign
    path = source / "manifest.json"
    manifest = json.loads(path.read_text())
    (source / "bin/bobbin").rename(source / "bin/bobbin.real")
    (source / "bin/bobbin").write_bytes(b"wrapper")
    runtime = {"device": "CUDA", "artifacts": [{"sha256": "2" * 64}]}
    (source / "bin/gpu-runtime.json").write_text(json.dumps(runtime))
    manifest.update(embedding_runtime=runtime,
                    gpu_guard_sha256=resume.digest(source / "bin/bobbin"))
    path.write_text(json.dumps(manifest))
    sha = resume.digest(path)
    assert resume.plan(source, root, sha)["runtime_library_lock_present"] is False
    (source / "bin/gpu-runtime.json").write_text('{}')
    with pytest.raises(ValueError, match="configuration differs"):
        resume.plan(source, root, sha)
    (source / "bin/gpu-runtime.json").write_text(json.dumps(runtime))
    (source / "bin/bobbin").write_bytes(b"changed wrapper")
    with pytest.raises(ValueError, match="executable digest mismatch"):
        resume.plan(source, root, sha)


def test_symlink_evidence_is_refused(campaign):
    source, root, sha = campaign
    directory = source / "task-000/none"
    directory.mkdir(parents=True)
    (directory / "stream.jsonl").symlink_to(source / "manifest.json")
    with pytest.raises(ValueError, match="symlink in prior evidence"):
        resume.plan(source, root, sha)


def test_original_source_lookup_does_not_turn_unavailable_into_absence(tmp_path):
    with pytest.raises(ValueError, match="original source unavailable"):
        resume.original_file(tmp_path, "1" * 40, "tasks/task.yaml")
    with pytest.raises(ValueError, match="invalid original harness commit"):
        resume.original_file(tmp_path, "--help", "tasks/task.yaml")
