"""Pure mocked tests: no model, network, indexing, or paid evaluation."""

from copy import deepcopy

import pytest

from runner.jev_replay import Candidate, replay
from runner.l0 import ArmScore


def make_data():
    return dict(  # noqa: C408 -- mirrors the replay keyword arguments
        task_id="fixture",
        prompt="repair parsing",
        candidates=[
            Candidate("a", "a.py", 1, "a\na\na", 1.0),
            Candidate("b", "b.py", 10, "b\nb\nb", 0.1),
        ],
        baseline=ArmScore(
            task_id="fixture",
            arm="full",
            budget=3,
            outcome="injected",
            chunks=[("a.py", 1, 3)],
            injected_lines=3,
            injected_tokens=22,
        ),
        gold={"b.py": [(10, 12)]},
        model_revision="jev-fixture-1",
        capture={
            "bobbin_sha256": "binary-pin",
            "harness_commit": "harness-pin",
            "checkout_commit": "parent-pin",
            "candidate_stage": "post-adjustment-pre-packing",
        },
    )


@pytest.fixture
def data():
    return make_data()


def fake(values, calls):
    def ask(state, questions):
        calls.append((deepcopy(state), deepcopy(questions)))
        return {
            "model": "jev-fixture-1",
            "answers": {q: {"type": "noul", "noul": values.get(q, 1.0)} for q in questions},
        }

    return ask


def test_j1_batches_relevance_without_gold_and_reranks(data):
    calls = []
    before = deepcopy(data)
    r = replay(**data, arm="jev-j1", alpha=1, ask=fake({"relevance_0": 0, "relevance_1": 1}, calls))
    assert data == before
    assert len(calls) == 1
    assert set(calls[0][1]) == {"relevance_0", "relevance_1"}
    assert set(calls[0][0]) == {"prompt", "candidates"}
    assert r["score"]["chunks"] == [("b.py", 10, 12)]
    assert r["score"]["density"] == r["score"]["hunk_recall"] == 1
    assert r["audit"]["plane"] == "quarantine"


def test_j3_asks_only_prompt_and_retains_exact_baseline(data):
    calls = []
    r = replay(**data, arm="jev-j3", ask=fake({}, calls))
    assert calls[0][0] == {"prompt": "repair parsing"}
    assert r["score"]["chunks"] == data["baseline"].chunks
    assert r["score"]["injected_tokens"] == 22


def test_prompt_rejection_is_a_skip_not_an_error(data):
    r = replay(**data, arm="jev-j3", ask=fake({"needs_context": 0.1}, []))
    assert r["status"] == "evaluated"
    assert r["score"]["outcome"] == "skipped"
    assert r["score"]["hunk_recall"] == 0
    assert r["score"]["chunks"] == []


@pytest.mark.parametrize("outcome", ["skipped", "error"])
def test_baseline_floor_cannot_be_overridden_and_spends_nothing(data, outcome):
    data["baseline"].outcome = outcome
    calls = []
    r = replay(**data, ask=fake({}, calls))
    assert not calls
    assert r["score"]["outcome"] == outcome


@pytest.mark.parametrize("bad", [None, "0.5", True, -0.1, 1.1, float("nan"), float("inf")])
def test_malformed_probability_degrades_without_scoring_fallback_as_jev(data, bad):
    r = replay(**data, arm="jev-j3", ask=fake({"needs_context": bad}, []))
    assert r["status"] == "degraded"
    assert r["score"]["outcome"] == "error"
    assert r["score"]["density"] is None
    assert r["fallback"]["chunks"] == data["baseline"].chunks


def test_unavailable_never_leaks_exception_or_looks_like_skip(data):
    def unavailable(*args):
        raise RuntimeError("secret-bearing upstream body")

    r = replay(**data, ask=unavailable)
    assert r["status"] == "degraded"
    assert "JevUnavailable" in r["score"]["detail"]
    assert "secret-bearing" not in str(r)


@pytest.mark.parametrize(
    "response",
    [
        {"model": "wrong", "answers": {}},
        {"model": "jev-fixture-1", "answers": {}},
        {"model": "jev-fixture-1", "answers": {"needs_context": {"type": "choice", "noul": 1}}},
    ],
)
def test_model_question_and_type_contracts(data, response):
    assert replay(**data, arm="jev-j3", ask=lambda *args: response)["status"] == "degraded"


def test_stable_ties_and_budget_truncation(data):
    data["baseline"].budget = 4
    r = replay(**data, arm="jev-j1", alpha=1, ask=fake({}, []))
    assert r["score"]["chunks"] == [("a.py", 1, 3), ("b.py", 10, 10)]
    assert r["score"]["injected_lines"] == 4


@pytest.mark.parametrize("change", ["stage", "revision", "count", "duplicate", "state"])
def test_input_refusals_precede_any_call(data, change):
    if change == "stage":
        data["capture"]["candidate_stage"] = "post-packing"
    if change == "revision":
        data["model_revision"] = "jev-latest"
    if change == "count":
        data["candidates"] *= 16
    if change == "duplicate":
        data["candidates"] *= 2
    if change == "state":
        data["prompt"] = "x" * 60_001
    calls = []
    with pytest.raises(ValueError):
        replay(**data, ask=fake({}, calls))
    assert not calls


def test_combined_gate_is_blind_and_rejection_avoids_relevance_spend(data):
    calls = []
    r = replay(**data, ask=fake({"needs_context": 0}, calls))
    assert r["score"]["outcome"] == "skipped"
    assert len(calls) == 1
    assert calls[0][0] == {"prompt": "repair parsing"}


def test_combined_batches_candidates_only_after_positive_gate(data):
    calls = []
    r = replay(**data, ask=fake({}, calls))
    assert r["status"] == "evaluated"
    assert len(calls) == 2
    assert calls[0][0] == {"prompt": "repair parsing"}
    assert "candidates" in calls[1][0]
    assert set(calls[1][1]) == {"relevance_0", "relevance_1"}


def test_recorded_cli_requires_exact_request_and_refuses_overwrite(data, tmp_path):
    import json
    from dataclasses import asdict

    from runner.jev_replay import main, request

    data["arm"] = "jev-j1"
    data["baseline"] = asdict(data["baseline"])
    state, questions = request(data["prompt"], data["candidates"], data["arm"])
    data["candidates"] = [asdict(c) for c in data["candidates"]]
    capture, responses, out = [
        tmp_path / name for name in ("capture.json", "responses.json", "out.json")
    ]
    capture.write_text(json.dumps(data))
    response = fake({}, [])(state, questions)
    records = [{"state": state, "questions": questions, "response": response}]
    responses.write_text(json.dumps(records))
    args = [str(capture), "--responses", str(responses), "--out", str(out)]
    assert main(args) == 0
    assert json.loads(out.read_text())["status"] == "evaluated"
    before = out.read_bytes()
    assert main(args) == 2
    assert out.read_bytes() == before
    records[0]["state"] = {"prompt": "another question"}
    responses.write_text(json.dumps(records))
    args[-1] = str(tmp_path / "mismatch.json")
    assert main(args) == 1
    assert json.loads((tmp_path / "mismatch.json").read_text())["status"] == "degraded"
