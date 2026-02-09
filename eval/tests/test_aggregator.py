"""Tests for scorer.aggregator."""

from __future__ import annotations

from scorer.aggregator import aggregate, aggregate_across_runs


class TestAggregate:
    def test_basic_aggregation(self):
        test_result = {"passed": True, "total": 10, "failures": 0}
        diff_result = {
            "file_precision": 0.8,
            "file_recall": 0.9,
            "f1": 0.85,
            "exact_file_match": False,
        }

        combined = aggregate(test_result, diff_result)

        assert combined["test_passed"] is True
        assert combined["test_total"] == 10
        assert combined["test_failures"] == 0
        assert combined["file_precision"] == 0.8
        assert combined["file_recall"] == 0.9
        assert combined["f1"] == 0.85
        assert combined["exact_file_match"] is False
        assert "judge" not in combined

    def test_with_judge_results(self):
        test_result = {"passed": True, "total": 5, "failures": 0}
        diff_result = {"file_precision": 1.0, "file_recall": 1.0, "f1": 1.0, "exact_file_match": True}
        judge_results = {
            "overall_winner": "b",
            "bias_detected": False,
            "dimensions": {
                "consistency": {"a": 3, "b": 4},
                "completeness": {"a": 3, "b": 5},
                "minimality": {"a": 4, "b": 4},
            },
        }

        combined = aggregate(test_result, diff_result, judge_results)

        assert "judge" in combined
        assert combined["judge"]["overall_winner"] == "b"
        assert combined["judge"]["bias_detected"] is False

    def test_missing_fields_default(self):
        combined = aggregate({}, {})
        assert combined["test_passed"] is False
        assert combined["test_total"] == 0
        assert combined["file_precision"] == 0.0

    def test_none_judge_results(self):
        combined = aggregate({"passed": True}, {"f1": 0.5}, None)
        assert "judge" not in combined


class TestAggregateAcrossRuns:
    def test_basic_stats(self):
        results = [
            {
                "test_result": {"passed": True},
                "diff_result": {"file_precision": 0.8, "file_recall": 0.6, "f1": 0.69},
                "agent_result": {"duration_seconds": 100.0},
            },
            {
                "test_result": {"passed": False},
                "diff_result": {"file_precision": 0.4, "file_recall": 0.8, "f1": 0.53},
                "agent_result": {"duration_seconds": 200.0},
            },
        ]

        stats = aggregate_across_runs(results)

        assert stats["count"] == 2
        assert stats["test_pass_rate"] == 0.5
        assert abs(stats["avg_file_precision"] - 0.6) < 0.01
        assert abs(stats["avg_file_recall"] - 0.7) < 0.01
        assert abs(stats["avg_duration_seconds"] - 150.0) < 0.01

    def test_empty_results(self):
        stats = aggregate_across_runs([])
        assert stats["count"] == 0
        assert stats["test_pass_rate"] == 0.0

    def test_with_judge_results(self):
        results = [
            {
                "test_result": {"passed": True},
                "diff_result": {"file_precision": 1.0, "file_recall": 1.0, "f1": 1.0},
                "agent_result": {"duration_seconds": 50.0},
                "judge_result": {"overall_winner": "b"},
            },
            {
                "test_result": {"passed": True},
                "diff_result": {"file_precision": 0.8, "file_recall": 0.8, "f1": 0.8},
                "agent_result": {"duration_seconds": 60.0},
                "judge_result": {"overall_winner": "a"},
            },
            {
                "test_result": {"passed": True},
                "diff_result": {"file_precision": 0.9, "file_recall": 0.9, "f1": 0.9},
                "agent_result": {"duration_seconds": 70.0},
                "judge_result": {"overall_winner": "b"},
            },
        ]

        stats = aggregate_across_runs(results)
        assert "judge_summary" in stats
        assert stats["judge_summary"]["count"] == 3
        assert stats["judge_summary"]["wins"]["b"] == 2
        assert stats["judge_summary"]["wins"]["a"] == 1

    def test_missing_nested_fields(self):
        results = [
            {"test_result": {"passed": True}},
            {},
        ]
        stats = aggregate_across_runs(results)
        assert stats["count"] == 2
        assert stats["test_pass_rate"] == 0.5
