"""Chained continuation boundaries, using only synthetic campaign artifacts."""
import hashlib
import json

import pytest

from runner import pilot_chain as chain
from runner import pilot_continue as continuation
from runner import pilot_resume as resume
from tests.test_pilot_continue import execute as run_first
from tests.test_pilot_continue import inputs as first_inputs  # noqa: F401
from tests.test_pilot_continue import write
from tests.test_pilot_resume import campaign as campaign_fixture  # noqa: F401


def spend(task, output, *args, before_spend):
    before_spend()
    write(output / task / "none/result.json", {"spent": task})


@pytest.fixture
def chained(first_inputs, tmp_path, monkeypatch):  # noqa: F811
    source, root = first_inputs[0], first_inputs[8]
    parent = run_first(first_inputs, tmp_path, monkeypatch, spend)
    # A repaired definition of a never-paid task is allowed and must be measured.
    (root / "tasks/task-003.yaml").write_text("id: task-003\nrepaired: true\n")
    tasks = [f"task-{i:03}" for i in range(2, 30)]
    report = tmp_path / "chain-controls/report.json"
    rows = [{"task": task, "eligible": task == "task-002",
             "status": "fixture_pass" if task == "task-002" else "fixture_exclusion"}
            for task in tasks]
    digests = {task: hashlib.sha256((root / "tasks" / f"{task}.yaml").read_bytes()).hexdigest()
               for task in tasks}
    write(report, {"kind": "fixture-controls-standalone", "all_tasks_checked": True,
                   "model_calls": 0, "task_sha256": digests, "tasks": rows,
                   "eligible_tasks": ["task-002"]})
    write(report.parent / "task-002/controls.json", {
        "parent": {"valid": True, "passed": False, "executed": 2},
        "fix": {"valid": True, "passed": True, "executed": 2}})
    monkeypatch.setattr(chain, "verify_runtime", lambda value: {"verified_libraries": 22})
    lock, client = first_inputs[4], first_inputs[6]
    evidence = chain.evidence_digest(chain.parent_evidence(parent, ["task-001"])[0])
    return [source, first_inputs[1], parent, resume.digest(parent / "manifest.json"), evidence,
            report, resume.digest(report), lock, resume.digest(lock),
            client, resume.digest(client), root]


def test_plan_skips_original_and_first_continuation_tasks(chained):
    planned = chain.build_plan(*chained)
    assert planned["schedule"] == [["task-002", c] for c in ("none", "tool", "inject", "both")]
    assert planned["parent"]["tasks"] == ["task-001"]
    assert "task-001/none/result.json" in planned["parent"]["artifact_sha256"]
    assert list(planned["recovery"]["amended_task_sha256"]) == ["task-003"]
    assert not (chained[2] / "continuation-claim.json").exists()


@pytest.mark.parametrize("damage", [
    "incomplete", "paid", "measured_other_definition", "missing_task", "parent_manifest",
    "parent_evidence", "parent_unknown", "runtime_pin", "client_pin", "chained_claim",
    "claim_elsewhere", "amended_paid_original", "amended_parent_task", "legacy_report"])
def test_refuses_changed_inputs(chained, damage):
    source, parent, report, root = chained[0], chained[2], chained[5], chained[11]
    data = json.loads(report.read_text())
    if damage == "incomplete":
        data["all_tasks_checked"] = False
    elif damage == "paid":
        data["model_calls"] = 1
    elif damage == "measured_other_definition":
        data["task_sha256"]["task-002"] = "0" * 64
    elif damage == "missing_task":
        data["tasks"].pop()
    elif damage == "legacy_report":
        data["kind"] = "fixture-controls-only"
    elif damage == "parent_manifest":
        (parent / "manifest.json").write_text("{}")
    elif damage == "parent_evidence":
        (parent / "task-001/none/result.json").write_text("rewritten")
    elif damage == "parent_unknown":
        (parent / "task-777").mkdir()
    elif damage == "runtime_pin":
        chained[8] = "f" * 64
    elif damage == "client_pin":
        chained[10] = "f" * 64
    elif damage == "chained_claim":
        (parent / "continuation-claim.json").write_text("{}")
    elif damage == "claim_elsewhere":
        claim = json.loads((source / "continuation-claim.json").read_text())
        claim["output"] = str(root)
        (source / "continuation-claim.json").write_text(json.dumps(claim))
    elif damage == "amended_paid_original":
        (root / "tasks/task-000.yaml").write_text("changed after paid evidence")
    else:
        (root / "tasks/task-001.yaml").write_text("changed after first continuation")
    write(report, data)
    if damage != "parent_manifest":
        chained[3] = resume.digest(parent / "manifest.json")
    chained[6] = resume.digest(report)
    with pytest.raises(ValueError):
        chain.build_plan(*chained)
    assert not (parent / "continuation-claim.json").exists() or damage == "chained_claim"


def execute(chained, tmp_path, monkeypatch, runner):
    planned = chain.build_plan(*chained)
    monkeypatch.setattr(continuation.pilot, "run_task", runner)
    output = tmp_path / "chained-out"
    chain.execute(planned, chained[0], chained[2], output, chained[7], chained[11],
                  lambda: chain.build_plan(*chained))
    return output


def test_runs_once_with_claim_in_first_continuation(chained, tmp_path, monkeypatch):
    called = []
    original_claim = (chained[0] / "continuation-claim.json").read_bytes()

    def run(task, output, *args, before_spend):
        before_spend()
        called.append(task)
    output = execute(chained, tmp_path, monkeypatch, run)
    assert called == ["task-002"]
    manifest = json.loads((output / "manifest.json").read_text())
    assert manifest["continuation"]["parent"]["tasks"] == ["task-001"]
    assert (chained[2] / "continuation-claim.json").is_file()
    assert (chained[0] / "continuation-claim.json").read_bytes() == original_claim
    with pytest.raises(ValueError, match="already has a chained claim"):
        chain.build_plan(*chained)


@pytest.mark.parametrize("damage", ["parent_evidence", "task_definition", "original_evidence"])
def test_rechecks_before_every_spend(chained, tmp_path, monkeypatch, damage):
    targets = {"parent_evidence": chained[2] / "task-001/none/result.json",
               "task_definition": chained[11] / "tasks/task-002.yaml",
               "original_evidence": chained[0] / "task-000/none/result.json"}

    def run(task, output, *args, before_spend):
        before_spend()
        targets[damage].write_text("changed between cells")
        before_spend()
        pytest.fail("second spend allowed")
    with pytest.raises(RuntimeError, match="stopped before spending"):
        execute(chained, tmp_path, monkeypatch, run)
    assert (chained[2] / "continuation-claim.json").is_file()


def test_output_inside_first_continuation_refused(chained):
    planned = chain.build_plan(*chained)
    with pytest.raises(ValueError, match="new and outside"):
        chain.execute(planned, chained[0], chained[2], chained[2] / "nested", chained[7],
                      chained[11], lambda: planned)
    assert not (chained[2] / "continuation-claim.json").exists()


def test_default_plan_still_refuses_amended_definitions(chained):
    with pytest.raises(ValueError, match="task definition changed"):
        resume.plan(chained[0], chained[11], chained[1])
