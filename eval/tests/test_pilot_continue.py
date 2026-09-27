"""Continuation spending boundaries, using only synthetic campaign artifacts."""
import json

import pytest

from runner import pilot_continue as continuation
from runner import pilot_resume as resume
from tests.test_pilot_resume import campaign as campaign_fixture  # noqa: F401


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


@pytest.fixture
def inputs(campaign_fixture, tmp_path, monkeypatch):  # noqa: F811
    source, root, _ = campaign_fixture
    manifest_path = source / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    (source / "bin/bobbin").rename(source / "bin/bobbin.real")
    (source / "bin/bobbin").write_bytes(b"old guard")
    runtime = {"artifacts": [{"sha256": "a" * 64}], "environment": {}}
    write(source / "bin/gpu-runtime.json", runtime)
    manifest.update(model=continuation.pilot.MODEL, embedding_runtime=runtime,
                    gpu_guard_sha256=resume.digest(source / "bin/bobbin"),
                    budget_per_run=2.0, timeout=900, max_turns=40)
    write(manifest_path, manifest)
    sha = resume.digest(manifest_path)
    # An already spent partial pair must never reach the continuation runner.
    write(source / "task-000/none/result.json", {"already_spent": True})
    recovery = resume.plan(source, root, sha)
    report = tmp_path / "controls/report.json"
    tasks = list(dict.fromkeys(task for task, _ in recovery["remaining_schedule"]))
    rows = [{"task": task, "eligible": task == "task-001",
             "status": "fixture_pass" if task == "task-001" else "fixture_exclusion"}
            for task in tasks]
    write(report, {"kind": "fixture-controls-only", "all_tasks_checked": True,
                   "model_calls": 0, "source_manifest_sha256": sha,
                   "tasks": rows, "eligible_tasks": ["task-001"]})
    write(report.parent / "recovery-plan.json", recovery)
    write(report.parent / "task-001/controls.json", {
        "parent": {"valid": True, "passed": False, "executed": 1},
        "fix": {"valid": True, "passed": True, "executed": 1}})
    lock = tmp_path / "lock.json"
    write(lock, {**runtime, "library_lock_version": 1})
    client = tmp_path / "client"
    client.write_bytes(b"pinned client")
    (root / "runner").mkdir()
    for name in ("gpu_guard.py", "runtime_lock.py"):
        (root / "runner" / name).write_text("# current guarded implementation\n")
    (root / "v2").mkdir()
    (root / "v2/PREREGISTRATION.md").write_text("recovery amendment")
    monkeypatch.setattr(continuation, "verify_runtime", lambda value: {"verified_libraries": 22})
    args = [source, sha, report, resume.digest(report), lock, resume.digest(lock),
            client, resume.digest(client), root]
    return args


def test_plan_never_invokes_or_claims_and_excludes_all_attempted_tasks(inputs, monkeypatch):
    monkeypatch.setattr(continuation.pilot, "run_task", lambda *a, **kw: pytest.fail("spent"))
    planned = continuation.build_plan(*inputs)
    assert planned["schedule"] == [["task-001", cell] for cell in ("none", "tool", "inject", "both")]
    assert planned["original_client_was_pinned"] is False
    assert not (inputs[0] / "continuation-claim.json").exists()


@pytest.mark.parametrize("damage", ["incomplete", "paid", "missing_task", "nondiscriminating",
                                   "runtime", "client", "evidence"])
def test_refuses_bad_controls_or_changed_pins(inputs, damage):
    report, lock, client = inputs[2], inputs[4], inputs[6]
    data = json.loads(report.read_text())
    if damage == "incomplete":
        data["all_tasks_checked"] = False
    elif damage == "paid":
        data["model_calls"] = 1
    elif damage == "missing_task":
        data["tasks"].pop()
    elif damage == "nondiscriminating":
        receipt = report.parent / "task-001/controls.json"
        controls = json.loads(receipt.read_text())
        controls["parent"]["passed"] = True
        write(receipt, controls)
    elif damage == "runtime":
        write(lock, {"artifacts": [{"sha256": "b" * 64}], "environment": {}})
        inputs[5] = resume.digest(lock)
    elif damage == "client":
        client.write_text("replacement")
    else:
        (inputs[0] / "task-000/none/result.json").write_text("changed")
    write(report, data)
    inputs[3] = resume.digest(report)
    with pytest.raises(ValueError):
        continuation.build_plan(*inputs)
    assert not (inputs[0] / "continuation-claim.json").exists()


def execute(inputs, tmp_path, monkeypatch, runner):
    planned = continuation.build_plan(*inputs)
    monkeypatch.setattr(continuation.pilot, "run_task", runner)
    monkeypatch.setattr(continuation.pilot, "verify_and_record", lambda *a: {"status": "verified"})
    # No actual repository subprocess needed in synthetic execution.
    monkeypatch.setattr(continuation.subprocess, "check_output", lambda *a, **k: "2" * 40)
    output = tmp_path / "continued"
    continuation.execute(planned, inputs[0], output, inputs[4], inputs[8],
                         lambda: continuation.build_plan(*inputs))
    return output


def test_single_claim_runs_only_eligible_untouched_task(inputs, tmp_path, monkeypatch):
    called = []
    def run(task, *args, before_spend):
        before_spend()
        called.append(task)
    output = execute(inputs, tmp_path, monkeypatch, run)
    assert called == ["task-001"]
    assert json.loads((output / "manifest.json").read_text())["continuation"]["schedule"]
    with pytest.raises(ValueError, match="already has a continuation claim"):
        continuation.build_plan(*inputs)
    assert (inputs[0] / "task-000/none/result.json").read_text() == '{"already_spent": true}'


@pytest.mark.parametrize("damage", ["client", "receipt", "report", "evidence", "binary"])
def test_rechecks_before_every_spend_and_keeps_claim_on_failure(inputs, tmp_path, monkeypatch, damage):
    def run(task, output, *args, before_spend):
        before_spend()  # First cell would be allowed.
        targets = {"client": inputs[6], "receipt": inputs[2].parent / "task-001/controls.json",
                   "report": inputs[2], "evidence": inputs[0] / "task-000/none/result.json",
                   "binary": output / "bin/bobbin.real"}
        targets[damage].write_text("changed between cells")
        before_spend()  # Must refuse a second cell.
        pytest.fail("second spend allowed")
    with pytest.raises(RuntimeError, match="stopped before spending"):
        execute(inputs, tmp_path, monkeypatch, run)
    assert (inputs[0] / "continuation-claim.json").is_file()
    with pytest.raises(ValueError, match="already has a continuation claim"):
        continuation.build_plan(*inputs)


def test_changed_revalidation_refuses_before_claim(inputs, tmp_path):
    planned = continuation.build_plan(*inputs)
    with pytest.raises(ValueError, match="changed before claim"):
        continuation.execute(planned, inputs[0], tmp_path / "out", inputs[4], inputs[8], dict)
    assert not (inputs[0] / "continuation-claim.json").exists()


def test_racing_claim_is_exclusive(inputs, tmp_path):
    planned = continuation.build_plan(*inputs)
    def competing_claim():
        (inputs[0] / "continuation-claim.json").write_text("other executor")
        return planned
    with pytest.raises(FileExistsError):
        continuation.execute(planned, inputs[0], tmp_path / "out", inputs[4], inputs[8], competing_claim)
    assert (inputs[0] / "continuation-claim.json").read_text() == "other executor"
    assert not (tmp_path / "out").exists()
