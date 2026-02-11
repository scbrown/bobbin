"""Tests for runner.cli."""

from __future__ import annotations

import json

from click.testing import CliRunner

from runner.cli import _extract_token_usage, cli


def _make_result(
    task_id: str = "ruff-001",
    approach: str = "no-bobbin",
    attempt: int = 0,
    passed: bool = True,
    precision: float = 0.75,
    recall: float = 0.80,
    f1: float = 0.77,
    duration: float = 120.5,
) -> dict:
    return {
        "task_id": task_id,
        "approach": approach,
        "attempt": attempt,
        "status": "completed",
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
    }


class TestCLIGroup:
    def test_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["--help"])
        assert result.exit_code == 0
        assert "Bobbin evaluation framework" in result.output

    def test_verbose_flag(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["-v", "--help"])
        assert result.exit_code == 0


class TestScoreCommand:
    def test_score_with_results(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        for i in range(3):
            (rdir / f"ruff-001_no-bobbin_{i}.json").write_text(
                json.dumps(_make_result(attempt=i, passed=(i < 2)))
            )
        for i in range(3):
            (rdir / f"ruff-001_with-bobbin_{i}.json").write_text(
                json.dumps(_make_result(approach="with-bobbin", attempt=i))
            )

        runner = CliRunner()
        result = runner.invoke(cli, ["score", str(rdir)])
        assert result.exit_code == 0
        assert "no-bobbin" in result.output
        assert "with-bobbin" in result.output

    def test_score_no_results(self, tmp_path):
        rdir = tmp_path / "empty"
        rdir.mkdir()
        runner = CliRunner()
        result = runner.invoke(cli, ["score", str(rdir)])
        assert result.exit_code != 0

    def test_score_nonexistent_dir(self, tmp_path):
        runner = CliRunner()
        result = runner.invoke(cli, ["score", str(tmp_path / "nope")])
        assert result.exit_code != 0


class TestReportCommand:
    def test_report_generates_file(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "result.json").write_text(json.dumps(_make_result()))

        output = tmp_path / "report.md"
        runner = CliRunner()
        result = runner.invoke(cli, ["report", str(rdir), "-o", str(output)])
        assert result.exit_code == 0
        assert output.exists()
        assert "Report written to" in result.output

    def test_report_default_output(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "result.json").write_text(json.dumps(_make_result()))

        runner = CliRunner()
        result = runner.invoke(cli, ["report", str(rdir)])
        assert result.exit_code == 0
        assert (rdir / "report.md").exists()

    def test_report_empty_dir(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        runner = CliRunner()
        result = runner.invoke(cli, ["report", str(rdir)])
        assert result.exit_code != 0


class TestRunTaskCommand:
    def test_run_task_nonexistent_task(self, tmp_path):
        tasks_dir = tmp_path / "tasks"
        tasks_dir.mkdir()

        runner = CliRunner()
        result = runner.invoke(cli, [
            "run-task", "nonexistent",
            "--tasks-dir", str(tasks_dir),
        ])
        assert result.exit_code != 0
        assert "Error" in result.output

    def test_run_task_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["run-task", "--help"])
        assert result.exit_code == 0
        assert "TASK_ID" in result.output
        assert "--attempts" in result.output


class TestExtractTokenUsage:
    def test_full_result(self):
        result = {
            "total_cost_usd": 1.23,
            "usage": {
                "input_tokens": 5000,
                "output_tokens": 2000,
                "cache_creation_input_tokens": 100,
                "cache_read_input_tokens": 300,
            },
            "num_turns": 7,
            "modelUsage": {"claude-opus-4-6": {"input": 5000, "output": 2000}},
        }
        usage = _extract_token_usage(result)
        assert usage is not None
        assert usage["total_cost_usd"] == 1.23
        assert usage["input_tokens"] == 5000
        assert usage["output_tokens"] == 2000
        assert usage["cache_creation_tokens"] == 100
        assert usage["cache_read_tokens"] == 300
        assert usage["num_turns"] == 7
        assert usage["model_usage"] == {"claude-opus-4-6": {"input": 5000, "output": 2000}}

    def test_none_input(self):
        assert _extract_token_usage(None) is None

    def test_non_dict_input(self):
        assert _extract_token_usage("not a dict") is None

    def test_empty_dict(self):
        """Empty dict is falsy — no token data to extract."""
        assert _extract_token_usage({}) is None

    def test_partial_usage(self):
        result = {
            "total_cost_usd": 0.50,
            "usage": {"input_tokens": 1000},
        }
        usage = _extract_token_usage(result)
        assert usage["total_cost_usd"] == 0.50
        assert usage["input_tokens"] == 1000
        assert usage["output_tokens"] == 0
        assert usage["cache_creation_tokens"] == 0
        assert usage["cache_read_tokens"] == 0

    def test_raw_text_fallback(self):
        """When JSON parsing fails, agent_runner stores {"raw_text": ...}."""
        result = {"raw_text": "some output"}
        usage = _extract_token_usage(result)
        assert usage is not None
        assert usage["total_cost_usd"] is None
        assert usage["input_tokens"] == 0


class TestRunAllCommand:
    def test_run_all_empty_tasks_dir(self, tmp_path):
        tasks_dir = tmp_path / "tasks"
        tasks_dir.mkdir()

        runner = CliRunner()
        result = runner.invoke(cli, [
            "run-all",
            "--tasks-dir", str(tasks_dir),
        ])
        assert result.exit_code != 0

    def test_run_all_help(self):
        runner = CliRunner()
        result = runner.invoke(cli, ["run-all", "--help"])
        assert result.exit_code == 0
        assert "--tasks-dir" in result.output
        assert "--attempts" in result.output
