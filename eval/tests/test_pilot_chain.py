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
    assert planned["max_spend_usd"] == {"cells": 4, "budget_per_cell": 2.0, "total": 8.0}
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


@pytest.fixture
def stopped(chained, tmp_path, monkeypatch):
    """A chained run that spent task-002 and stopped in task-003's setup."""
    root = chained[11]
    # task-003 is eligible too, so the stopped run had two tasks scheduled.
    report = chained[5]
    data = json.loads(report.read_text())
    for row in data["tasks"]:
        if row["task"] == "task-003":
            row.update(eligible=True, status="fixture_pass")
    data["eligible_tasks"] = ["task-002", "task-003"]
    write(report, data)
    write(report.parent / "task-003/controls.json", {
        "parent": {"valid": True, "passed": False, "executed": 1},
        "fix": {"valid": True, "passed": True, "executed": 1}})
    chained[6] = resume.digest(report)
    (root / "runner/gpu_guard.py").write_text("# current guarded implementation\n")

    def run(task, output, *args, before_spend):
        if task == "task-002":
            before_spend()
            write(output / task / "none/result.json", {"spent": task})
            write(output / task / "controls.json", {})
            return
        write(output / task / "controls.json", {})
        write(output / task / "infrastructure-error.json", {"error": "init timed out"})
        raise RuntimeError("stopped in setup")
    with pytest.raises(RuntimeError, match="stopped in setup"):
        execute(chained, tmp_path, monkeypatch, run)
    chain_out = tmp_path / "chained-out"
    return chained, chain_out, resume.digest(chain_out / "manifest.json")


def resume_args(stopped):
    chained, chain_out, sha = stopped
    return [chain_out, sha, *chained]


def test_resume_skips_spent_and_reattempts_unspent_task(stopped):
    planned = chain.build_resume_plan(*resume_args(stopped))
    assert planned["resume"]["spent_tasks"] == ["task-002"]
    assert planned["resume"]["reattempted_unspent_tasks"] == ["task-003"]
    assert planned["schedule"] == [["task-003", c] for c in ("none", "tool", "inject", "both")]
    assert planned["max_spend_usd"] == {"cells": 4, "budget_per_cell": 2.0, "total": 8.0}
    assert "runtime_verifier_sha256_after" in planned["resume"]


@pytest.mark.parametrize("damage", ["cell_dir", "stream", "unknown_file", "unknown_top",
                                    "manifest", "guard", "controls", "claimed"])
def test_resume_refuses(stopped, damage):
    chained, chain_out, sha = stopped
    if damage == "cell_dir":
        # A cell directory means a spend may have happened: task-003 must be skipped,
        # and with nothing left the resume refuses rather than re-running it.
        (chain_out / "task-003/none").mkdir()
    elif damage == "stream":
        (chain_out / "task-003/stream.jsonl").write_text("{}")
    elif damage == "unknown_file":
        (chain_out / "task-003/notes.txt").write_text("?")
    elif damage == "unknown_top":
        (chain_out / "pilot-leftover").mkdir()
    elif damage == "manifest":
        sha = "0" * 64
    elif damage == "guard":
        (chained[11] / "runner/gpu_guard.py").write_text("# a different wrapper\n")
    elif damage == "controls":
        (chained[5].parent / "task-003/controls.json").write_text("changed")
    else:
        (chain_out / "continuation-claim.json").write_text("{}")
    with pytest.raises(ValueError):
        chain.build_resume_plan(chain_out, sha, *chained)


def test_resume_runs_once_with_claim_in_the_stopped_run(stopped, tmp_path, monkeypatch):
    chained, chain_out, sha = stopped
    args = resume_args(stopped)
    planned = chain.build_resume_plan(*args)
    called = []

    def run(task, output, *a, before_spend):
        before_spend()
        called.append(task)
    monkeypatch.setattr(continuation.pilot, "run_task", run)
    out = tmp_path / "resumed"
    chain.execute(planned, chained[0], chained[2], out, chained[7], chained[11],
                  lambda: chain.build_resume_plan(*args))
    assert called == ["task-003"]
    assert (chain_out / "continuation-claim.json").is_file()
    assert json.loads((out / "manifest.json").read_text())["continuation"]["resume"]["spent_tasks"] == ["task-002"]
    with pytest.raises(ValueError, match="already has a resume claim"):
        chain.build_resume_plan(*args)


def test_resume_rechecks_stopped_run_evidence_before_spend(stopped, tmp_path, monkeypatch):
    chained, chain_out, sha = stopped
    args = resume_args(stopped)
    planned = chain.build_resume_plan(*args)

    def run(task, output, *a, before_spend):
        before_spend()
        (chain_out / "task-002/none/result.json").write_text("rewritten")
        before_spend()
        pytest.fail("second spend allowed")
    monkeypatch.setattr(continuation.pilot, "run_task", run)
    with pytest.raises(RuntimeError, match="stopped before spending"):
        chain.execute(planned, chained[0], chained[2], tmp_path / "resumed", chained[7],
                      chained[11], lambda: chain.build_resume_plan(*args))


INFRA = {"agent": {"serving_model": None, "valid": False,
                   "result": {"terminal_reason": "api_error", "total_cost_usd": 0, "modelUsage": {},
                              "result": "Failed to authenticate: OAuth session expired"}}}


@pytest.fixture
def resumed_stop(stopped, tmp_path, monkeypatch):
    """A first resume whose only cell failed because the agent never reached a model."""
    chained, chain_out, sha = stopped
    args = resume_args(stopped)
    planned = chain.build_resume_plan(*args)

    def run(task, output, *a, before_spend):
        before_spend()
        write(output / task / "controls.json", {})
        write(output / task / "tool/result.json", INFRA)
        raise RuntimeError("agent unavailable")
    monkeypatch.setattr(continuation.pilot, "run_task", run)
    out = tmp_path / "resumed"
    with pytest.raises(RuntimeError, match="agent unavailable"):
        chain.execute(planned, chained[0], chained[2], out, chained[7], chained[11],
                      lambda: chain.build_resume_plan(*args))
    return stopped, out, resume.digest(out / "manifest.json")


def second_args(resumed_stop, cells=("task-003/tool",)):
    (chained, chain_out, chain_sha), out, sha = resumed_stop
    return [out, sha, list(cells), chain_out, chain_sha, *chained]


def test_second_resume_reattempts_infra_cell_task_once(resumed_stop):
    planned = chain.build_second_resume_plan(*second_args(resumed_stop))
    second = planned["second_resume"]
    assert second["spent_tasks"] == []
    assert [(r["task"], r["cell"]) for r in second["reattempted_infra_cells"]] == [("task-003", "tool")]
    assert planned["schedule"] == [["task-003", c] for c in ("none", "tool", "inject", "both")]
    assert planned["max_spend_usd"]["total"] == 8.0
    assert planned["resume"]["spent_tasks"] == ["task-002"]


@pytest.mark.parametrize("damage", ["reached_model", "cost", "unlisted_cell", "not_spent",
                                    "bad_cell", "no_list", "guard", "verifier", "manifest",
                                    "claimed", "evidence"])
def test_second_resume_refuses(resumed_stop, damage):
    (chained, chain_out, _), out, _ = resumed_stop
    args = second_args(resumed_stop)
    root = chained[11]
    receipt = out / "task-003/tool/result.json"
    if damage == "reached_model":
        data = json.loads(receipt.read_text())
        data["agent"]["serving_model"] = "claude-sonnet-5"
        write(receipt, data)
    elif damage == "cost":
        data = json.loads(receipt.read_text())
        data["agent"]["result"]["total_cost_usd"] = 0.4
        write(receipt, data)
    elif damage == "unlisted_cell":
        write(out / "task-003/none/result.json", INFRA)
    elif damage == "not_spent":
        args[2] = ["task-002/none"]
    elif damage == "bad_cell":
        args[2] = ["task-003/extra"]
    elif damage == "no_list":
        args[2] = []  # the spent task is then skipped and nothing remains
    elif damage == "guard":
        (root / "runner/gpu_guard.py").write_text("# a different wrapper\n")
    elif damage == "verifier":
        (root / "runner/runtime_lock.py").write_text("# a different verifier\n")
    elif damage == "manifest":
        args[1] = "0" * 64
    elif damage == "claimed":
        (out / "continuation-claim.json").write_text("{}")
    else:
        (chain_out / "task-002/none/result.json").write_text("rewritten")
    with pytest.raises(ValueError):
        chain.build_second_resume_plan(*args)


def test_second_resume_runs_once_with_claim_in_the_resume_run(resumed_stop, tmp_path, monkeypatch):
    (chained, _, _), out, _ = resumed_stop
    args = second_args(resumed_stop)
    planned = chain.build_second_resume_plan(*args)
    called = []

    def run(task, output, *a, before_spend):
        before_spend()
        called.append(task)
    monkeypatch.setattr(continuation.pilot, "run_task", run)
    final = tmp_path / "resumed-2"
    chain.execute(planned, chained[0], chained[2], final, chained[7], chained[11],
                  lambda: chain.build_second_resume_plan(*args))
    assert called == ["task-003"]
    assert (out / "continuation-claim.json").is_file()
    assert (out / "task-003/tool/result.json").is_file()  # prior receipt kept as evidence
    with pytest.raises(ValueError, match="already has a second-resume claim"):
        chain.build_second_resume_plan(*args)


def test_second_resume_rechecks_resume_run_evidence_before_spend(resumed_stop, tmp_path, monkeypatch):
    (chained, _, _), out, _ = resumed_stop
    args = second_args(resumed_stop)
    planned = chain.build_second_resume_plan(*args)

    def run(task, output, *a, before_spend):
        before_spend()
        (out / "task-003/tool/result.json").write_text("rewritten")
        before_spend()
        pytest.fail("second spend allowed")
    monkeypatch.setattr(continuation.pilot, "run_task", run)
    with pytest.raises(RuntimeError, match="stopped before spending"):
        chain.execute(planned, chained[0], chained[2], tmp_path / "resumed-2", chained[7],
                      chained[11], lambda: chain.build_second_resume_plan(*args))
