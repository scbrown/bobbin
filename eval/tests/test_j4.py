"""Tests for J4 (runner.j4), PREREGISTRATION amendment A1.

Pure logic with a fake judge and a synthetic workspace. Nothing here calls Jev or scores the
task set.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from runner import j4, l0

TASK_IDS = [f"{r}-{i:03d}" for r in j4.REPO_CYCLE for i in range(1, 6)]


# --- split and negative set --------------------------------------------------


def test_committed_calibration_split_matches_the_registered_procedure():
    committed = j4.calibration_ids()
    assert committed == j4.calibration_split(TASK_IDS)
    assert len(committed) == 16
    # Two per repo, and NOT the same positions in every repo (the per-repo-RNG flaw).
    assert {t.rsplit("-", 1)[0] for t in committed} == set(j4.REPO_CYCLE)
    assert len({t.rsplit("-", 1)[1] for t in committed}) > 2


def test_negative_partner_cycles_repos_and_wraps():
    assert j4.negative_partner("cargo-003", TASK_IDS) == "django-003"
    assert j4.negative_partner("typst-005", TASK_IDS) == "cargo-005"
    assert (
        j4.negative_partner("cargo-009", TASK_IDS) is None
    )  # missing is reported, not substituted


# --- judging ------------------------------------------------------------------


@pytest.fixture
def ws(tmp_path: Path) -> Path:
    (tmp_path / "a.py").write_text("\n".join(f"line {i}" for i in range(1, 301)))
    return tmp_path


def test_state_is_fixed_shape_and_passage_is_capped(ws):
    state = j4.build_state("  fix the thing ", ws, ("a.py", 1, 300))
    assert state.startswith("Task: fix the thing\n\nPassage (a.py:1-300):\nline 1\n")
    assert state.count("\n") == 3 + j4.PASSAGE_MAX_LINES - 1


def test_candidates_are_capped_and_every_one_is_judged(ws):
    cands = [("a.py", i, i) for i in range(1, 31)]
    rec = j4.judge_task(
        "t", "positive", "t", "p", ws, "injected", cands, lambda s, q: (0.5, j4.JEV_MODEL)
    )
    assert len(rec.candidates) == j4.MAX_CANDIDATES == len(rec.probs)
    assert rec.model == j4.JEV_MODEL


def test_one_unjudged_candidate_makes_the_whole_task_an_error(ws, monkeypatch):
    monkeypatch.setattr(j4.time, "sleep", lambda _s: None)

    def judge(state, q):
        if "(a.py:2-2)" in state:
            raise j4.JevUnavailable("HTTP 500")
        return 0.9, j4.JEV_MODEL

    rec = j4.judge_task(
        "t", "positive", "t", "p", ws, "injected", [("a.py", 1, 1), ("a.py", 2, 2)], judge
    )
    assert rec.candidate_outcome == "error" and rec.probs == []
    assert j4.admit(rec, 0.1, 300) == ("error", [])


def test_retries_are_bounded():
    calls = []

    def judge(state, q):
        calls.append(1)
        raise j4.JevUnavailable("down")

    with pytest.raises(j4.JevUnavailable):
        j4.judge_with_retries(judge, "s", sleep=lambda _s: None)
    assert len(calls) == 1 + j4.JUDGE_RETRIES


def test_a_transient_failure_recovers_within_the_retries():
    answers = iter([j4.JevUnavailable("blip"), (0.7, j4.JEV_MODEL)])

    def judge(state, q):
        a = next(answers)
        if isinstance(a, Exception):
            raise a
        return a

    assert j4.judge_with_retries(judge, "s", sleep=lambda _s: None) == (0.7, j4.JEV_MODEL)


def test_mixed_or_foreign_model_versions_are_refused():
    ok = j4.Judged("t", "positive", "t", "injected", model=j4.JEV_MODEL)
    j4.refuse_mixed_models([ok])
    other = j4.Judged("u", "positive", "u", "injected", model="jev-1.14.0")
    with pytest.raises(ValueError, match="refusing to score"):
        j4.refuse_mixed_models([ok, other])


def test_missing_key_fails_loud(monkeypatch):
    monkeypatch.delenv("TYPESAFE_API_KEY", raising=False)
    monkeypatch.delenv("TYPESAFE_API_KEY_FILE", raising=False)
    with pytest.raises(j4.JevUnavailable):
        j4.resolve_key()


# --- admission and metrics ------------------------------------------------------


def rec_with(probs, outcome="injected"):
    cands = [("a.py", 10 * i + 1, 10 * i + 10) for i in range(len(probs))]
    return j4.Judged("t", "positive", "t", outcome, cands, list(probs), j4.JEV_MODEL)


def test_admission_keeps_order_and_abstains_as_its_own_outcome():
    rec = rec_with([0.9, 0.2, 0.8])
    assert j4.admit(rec, 0.5, 300) == ("injected", [("a.py", 1, 10), ("a.py", 21, 30)])
    assert j4.admit(rec, 0.95, 300) == ("abstained", [])
    assert j4.admit(rec_with([]), 0.5, 300) == ("skipped", [])
    assert j4.admit(rec_with([0.9], outcome="error"), 0.5, 300) == ("error", [])


def test_chunk_precision_is_undefined_not_zero_when_nothing_is_injected():
    gold = {"a.py": [(5, 6)]}
    assert j4.chunk_precision_at_k([], gold, 5) is None
    assert j4.chunk_precision_at_k([("a.py", 1, 10), ("b.py", 1, 10)], gold, 5) == 0.5
    assert j4.chunk_precision_at_k([("a.py", 1, 10), ("b.py", 1, 10)], gold, 1) == 1.0


def test_abstention_rate_excludes_errors_from_both_sides():
    assert j4.abstention_rate(["abstained", "injected", "error", "skipped"]) == pytest.approx(2 / 3)
    assert j4.abstention_rate(["error"]) is None


def _score(chunks, gold):
    return l0.score_chunks(chunks, gold)


def test_floor_rule_counts_abstentions_as_zero_recall_and_prefers_density():
    gold = {"a.py": [(1, 10)]}
    # Candidate 1 is gold and confident; candidate 2 is noise at 0.5.
    rows = [(rec_with([0.9, 0.5]), gold)] * 3
    floor, curve = j4.choose_floor(rows, full_mean_recall=1.0, score=_score)
    assert floor == 0.55  # the lowest floor that drops the noise chunk, recall still 1.0
    assert all(c["n"] == 3 for c in curve)
    top = next(c for c in curve if c["floor"] == 0.95)
    assert top["abstained"] == 3 and top["mean_recall"] == 0.0 and top["mean_density"] is None


def test_no_admissible_floor_is_reported_never_relaxed():
    gold = {"a.py": [(1, 10)]}
    rows = [(rec_with([0.0, 0.9]), gold)]  # the gold chunk is below every grid floor
    floor, _curve = j4.choose_floor(rows, full_mean_recall=1.0, score=_score)
    assert floor is None
