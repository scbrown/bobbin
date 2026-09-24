"""Exact packing oracle and semantic-replanning properties; no model calls."""

import itertools
import random

import pytest

from runner.jev_packing import knapsack
from runner.jev_replay import Candidate, replay
from tests.test_jev_replay import fake, make_data


@pytest.fixture
def data():
    return make_data()


def test_knapsack_matches_exhaustive_oracle():
    rng = random.Random(20260924)
    for _ in range(150):
        costs = [rng.randint(1, 8) for _ in range(rng.randint(0, 7))]
        values = [rng.randint(0, 8) / 8 for _ in costs]
        budget = rng.randint(0, 15)
        picked = knapsack(costs, values, budget)
        optimum = max(
            sum(values[i] for i in range(len(costs)) if mask[i])
            for mask in itertools.product((0, 1), repeat=len(costs))
            if sum(costs[i] for i in range(len(costs)) if mask[i]) <= budget
        )
        assert sum(values[i] for i in picked) == optimum
        assert sum(costs[i] for i in picked) <= budget
        assert len(set(picked)) == len(picked)


def test_knapsack_beats_greedy_density_and_preserves_ties():
    assert knapsack([3, 4, 5], [0.5, 0.6, 0.7], 9) == [1, 2]
    assert knapsack([2, 2, 2], [1, 1, 1], 4) == [0, 1]
    assert knapsack([1], [0], 9) == []


@pytest.mark.parametrize(
    "costs,values,budget",
    [
        ([0], [1], 3),
        ([1], [float("nan")], 3),
        ([1], [1], 601),
        ([1], [], 3),
    ],
)
def test_knapsack_refuses_invalid_inputs(costs, values, budget):
    with pytest.raises(ValueError):
        knapsack(costs, values, budget)


def test_novelty_is_reasked_after_selection_and_removes_semantic_duplicate(data):
    data["baseline"].budget = 6
    data["candidates"] = [Candidate(str(i), f"{i}.py", 1, "a\nb\nc", 1) for i in range(3)]
    calls = []

    def ask(state, questions):
        values = {}
        if state.get("selected"):
            # candidate1 is a paraphrase of selected candidate0; candidate2 is new.
            values["novelty_1"] = 0.0
        return fake(values, calls)(state, questions)

    r = replay(**data, arm="jev-j2", ask=ask, prior_context=["previous session fact"])
    assert r["score"]["chunks"] == [("0.py", 1, 3), ("2.py", 1, 3)]
    assert len(calls) == 3  # relevance+density, novelty empty, novelty after first chunk
    assert calls[1][0]["prior_context"] == ["previous session fact"]
    assert calls[2][0]["selected"][0]["id"] == "0"


def test_low_density_and_oversized_chunks_are_dropped_without_trimming(data):
    data["candidates"][0] = Candidate("a", "a.py", 1, "a\na\na\na", 1)
    r = replay(**data, arm="jev-j2", ask=fake({"density_1": 0.1}, []))
    assert r["status"] == "evaluated"
    assert r["score"]["outcome"] == "skipped"
    assert r["score"]["chunks"] == []


def test_exact_overlap_is_removed_even_if_model_claims_novelty(data):
    data["baseline"].budget = 6
    data["candidates"][1] = Candidate("b", "a.py", 2, "a\na\na", 1)
    calls = []
    r = replay(**data, arm="jev-j2", ask=fake({}, calls))
    assert r["score"]["chunks"] == [("a.py", 1, 3)]
    assert len(calls) == 2


def test_mid_pack_failure_retains_whole_baseline_not_partial_selection(data):
    data["baseline"].budget = 6
    calls = []

    def ask(state, questions):
        if state.get("selected"):
            raise TimeoutError("private transport details")
        return fake({}, calls)(state, questions)

    r = replay(**data, arm="jev-j2", ask=ask)
    assert r["status"] == "degraded"
    assert r["score"]["outcome"] == "error"
    assert r["score"]["chunks"] == []
    assert r["fallback"]["chunks"] == data["baseline"].chunks


def test_all_stages_keep_prompt_gate_blind(data):
    calls = []
    r = replay(**data, arm="jev-j1-j2-j3", ask=fake({}, calls))
    assert r["status"] == "evaluated"
    assert calls[0][0] == {"prompt": "repair parsing"}
    assert len(calls) <= len(data["candidates"]) + 2
