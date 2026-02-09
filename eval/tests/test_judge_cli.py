"""Tests for the judge CLI command and report judge integration."""

from __future__ import annotations

import json

from click.testing import CliRunner

from runner.cli import cli


def _make_result(
    task_id: str = "ruff-001",
    approach: str = "no-bobbin",
    attempt: int = 0,
    passed: bool = True,
    precision: float = 0.75,
    recall: float = 0.80,
    f1: float = 0.77,
    duration: float = 120.5,
    agent_diff: str = "diff --git a/src/a.rs b/src/a.rs\n+new line\n",
    ground_truth_diff: str = "diff --git a/src/a.rs b/src/a.rs\n+correct line\n",
) -> dict:
    return {
        "task_id": task_id,
        "approach": approach,
        "attempt": attempt,
        "status": "completed",
        "task": {
            "repo": "astral-sh/ruff",
            "commit": "abc123",
            "test_command": "cargo test",
            "language": "rust",
            "difficulty": "medium",
        },
        "agent_result": {
            "exit_code": 0,
            "duration_seconds": duration,
            "timed_out": False,
        },
        "test_result": {
            "passed": passed,
            "total": 10,
            "failures": 0 if passed else 2,
            "parsed": {},
        },
        "diff_result": {
            "file_precision": precision,
            "file_recall": recall,
            "f1": f1,
            "files_touched": ["src/a.rs"],
            "ground_truth_files": ["src/a.rs"],
            "exact_file_match": True,
        },
        "agent_diff": agent_diff,
        "ground_truth_diff": ground_truth_diff,
    }


class TestJudgeCommand:
    def test_judge_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["judge", "--help"])
        assert result.exit_code == 0
        assert "pairwise comparison" in result.output
        assert "--judge-model" in result.output
        assert "--pairs" in result.output

    def test_judge_nonexistent_dir(self, tmp_path):
        runner = CliRunner()
        result = runner.invoke(cli, ["judge", str(tmp_path / "nope")])
        assert result.exit_code != 0

    def test_judge_empty_dir(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        runner = CliRunner()
        result = runner.invoke(cli, ["judge", str(rdir)])
        assert result.exit_code != 0

    def test_judge_no_diffs_in_results(self, tmp_path):
        """Results without stored diffs should fail gracefully."""
        rdir = tmp_path / "results"
        rdir.mkdir()
        # Write result without agent_diff field.
        result_data = _make_result()
        del result_data["agent_diff"]
        del result_data["ground_truth_diff"]
        (rdir / "ruff-001_no-bobbin_0.json").write_text(json.dumps(result_data))

        runner = CliRunner()
        result = runner.invoke(cli, ["judge", str(rdir)])
        assert result.exit_code != 0
        assert "do not contain stored diffs" in result.output

    def test_judge_skips_empty_diffs(self, tmp_path):
        """Pairs with empty diffs should be skipped."""
        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "ruff-001_no-bobbin_0.json").write_text(
            json.dumps(_make_result(agent_diff="", ground_truth_diff=""))
        )

        runner = CliRunner()
        result = runner.invoke(cli, ["judge", str(rdir)])
        # Should complete but produce no judgements (skips empty diffs).
        assert "No judgements" in result.output or "Skipping" in result.output


class TestGroupResults:
    def test_group_and_pick_best(self, tmp_path):
        """Verify _group_results_by_task and _pick_best_attempt work."""
        from runner.cli import _group_results_by_task, _pick_best_attempt

        results = [
            _make_result(attempt=0, passed=False, f1=0.3),
            _make_result(attempt=1, passed=True, f1=0.7),
            _make_result(attempt=2, passed=True, f1=0.9),
        ]

        grouped = _group_results_by_task(results)
        assert "ruff-001" in grouped
        assert "no-bobbin" in grouped["ruff-001"]
        assert len(grouped["ruff-001"]["no-bobbin"]) == 3

        best = _pick_best_attempt(grouped["ruff-001"]["no-bobbin"])
        # Should pick attempt 2 (passing, highest F1).
        assert best["attempt"] == 2
        assert best["diff_result"]["f1"] == 0.9

    def test_pick_best_all_failing(self):
        """When no runs pass, pick highest F1 among failures."""
        from runner.cli import _pick_best_attempt

        results = [
            _make_result(attempt=0, passed=False, f1=0.2),
            _make_result(attempt=1, passed=False, f1=0.5),
        ]
        best = _pick_best_attempt(results)
        assert best["attempt"] == 1

    def test_pick_best_empty(self):
        from runner.cli import _pick_best_attempt
        assert _pick_best_attempt([]) is None


class TestReportWithJudge:
    def test_report_includes_judge_section(self, tmp_path):
        """Report should include judge results when judge_results.json exists."""
        from analysis.report import generate_report

        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "ruff-001_no-bobbin_0.json").write_text(json.dumps(_make_result()))

        # Write judge results.
        judge_data = [
            {
                "task_id": "ruff-001",
                "pair": "ai-vs-ai+bobbin",
                "a_label": "no-bobbin",
                "b_label": "with-bobbin",
                "overall_winner": "b",
                "named_winner": "with-bobbin",
                "dimensions": {
                    "consistency": {"a": 3, "b": 4},
                    "completeness": {"a": 3, "b": 5},
                    "minimality": {"a": 4, "b": 4},
                },
                "bias_detected": False,
                "reasoning": "B is better",
            },
        ]
        (rdir / "judge_results.json").write_text(json.dumps(judge_data))

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "LLM Judge Results" in content
        assert "no-bobbin vs with-bobbin" in content
        assert "with-bobbin" in content

    def test_report_without_judge(self, tmp_path):
        """Report should work fine without judge results."""
        from analysis.report import generate_report

        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "ruff-001_no-bobbin_0.json").write_text(json.dumps(_make_result()))

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "LLM Judge Results" not in content
        assert "Summary" in content
