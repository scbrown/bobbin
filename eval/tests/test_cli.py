"""Tests for runner.cli."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from click.testing import CliRunner

from runner.cli import (
    _extract_cost_metrics,
    _extract_output_summary,
    _extract_token_usage,
    _override_approach_name,
    _parse_override_specs,
    _read_bobbin_metrics,
    _save_raw_stream,
    cli,
)


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
            "output": "",
            "exit_code": 0 if passed else 1,
            "timed_out": False,
        },
        "diff_result": {
            "file_precision": precision,
            "file_recall": recall,
            "f1": f1,
            "files_touched": ["src/a.rs"],
            "ground_truth_files": ["src/a.rs"],
            "exact_file_match": True,
        },
        "tool_use_summary": {
            "by_tool": {"Read": 3, "Edit": 2, "Bash": 1},
            "tool_sequence": ["Read", "Read", "Bash", "Edit", "Read", "Edit"],
            "first_edit_turn": 2,
            "bobbin_commands": [],
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


class TestParseOverrideSpecs:
    def test_single_override(self):
        variants = _parse_override_specs(("semantic_weight=0.0",))
        assert len(variants) == 1
        assert variants[0] == {"semantic_weight": "0.0"}

    def test_multiple_overrides(self):
        variants = _parse_override_specs((
            "semantic_weight=0.0",
            "gate_threshold=1.0",
            "coupling_depth=0",
        ))
        assert len(variants) == 3
        assert variants[0] == {"semantic_weight": "0.0"}
        assert variants[1] == {"gate_threshold": "1.0"}
        assert variants[2] == {"coupling_depth": "0"}

    def test_empty_tuple(self):
        assert _parse_override_specs(()) == []

    def test_invalid_key_raises(self):
        with pytest.raises(ValueError, match="Unknown override key"):
            _parse_override_specs(("invalid_key=42",))


class TestOverrideApproachName:
    def test_single_key(self):
        name = _override_approach_name({"semantic_weight": "0.0"})
        assert name == "with-bobbin+semantic_weight=0.0"

    def test_multiple_keys_sorted(self):
        name = _override_approach_name({"gate_threshold": "1.0", "doc_demotion": "0.3"})
        assert name == "with-bobbin+doc_demotion=0.3,gate_threshold=1.0"


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
        assert "--save-stream" in result.output
        assert "--config-overrides" in result.output


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
        assert "--save-stream" in result.output
        assert "--config-overrides" in result.output


class TestReadBobbinMetrics:
    """Tests for _read_bobbin_metrics gate_skip_details extraction."""

    def _write_metrics(self, ws: Path, source: str, events: list[dict]) -> None:
        bobbin_dir = ws / ".bobbin"
        bobbin_dir.mkdir(parents=True, exist_ok=True)
        lines = [json.dumps({"source": source, **e}) for e in events]
        (bobbin_dir / "metrics.jsonl").write_text("\n".join(lines))

    def test_gate_skip_details_extracted(self, tmp_path):
        self._write_metrics(tmp_path, "run1", [
            {
                "event_type": "hook_gate_skip",
                "metadata": {
                    "query": "how does auth work",
                    "top_score": 0.42,
                    "gate_threshold": 0.75,
                },
            },
            {
                "event_type": "hook_gate_skip",
                "metadata": {
                    "query": "fix login bug",
                    "top_score": 0.31,
                    "gate_threshold": 0.75,
                },
            },
        ])
        result = _read_bobbin_metrics(tmp_path, "run1", [])
        assert result is not None
        assert result["gate_skip_count"] == 2
        assert len(result["gate_skip_details"]) == 2
        assert result["gate_skip_details"][0] == {
            "query": "how does auth work",
            "top_score": 0.42,
            "gate_threshold": 0.75,
        }
        assert result["gate_skip_details"][1]["query"] == "fix login bug"

    def test_gate_skip_details_empty_when_no_skips(self, tmp_path):
        self._write_metrics(tmp_path, "run1", [
            {
                "event_type": "hook_injection",
                "metadata": {"files_returned": ["src/main.py"]},
            },
        ])
        result = _read_bobbin_metrics(tmp_path, "run1", [])
        assert result is not None
        assert result["gate_skip_count"] == 0
        assert result["gate_skip_details"] == []

    def test_gate_skip_details_missing_metadata(self, tmp_path):
        self._write_metrics(tmp_path, "run1", [
            {"event_type": "hook_gate_skip", "metadata": {}},
        ])
        result = _read_bobbin_metrics(tmp_path, "run1", [])
        assert result is not None
        assert result["gate_skip_details"] == [
            {"query": "", "top_score": None, "gate_threshold": None},
        ]

    def test_returns_none_when_no_metrics_file(self, tmp_path):
        result = _read_bobbin_metrics(tmp_path, "run1", [])
        assert result is None

    def test_filters_by_source_tag(self, tmp_path):
        self._write_metrics(tmp_path, "run1", [
            {
                "event_type": "hook_gate_skip",
                "metadata": {"query": "q1", "top_score": 0.3, "gate_threshold": 0.75},
            },
        ])
        # Append an event with a different source.
        with open(tmp_path / ".bobbin" / "metrics.jsonl", "a") as f:
            f.write("\n" + json.dumps({
                "source": "run2",
                "event_type": "hook_gate_skip",
                "metadata": {"query": "q2", "top_score": 0.5, "gate_threshold": 0.75},
            }))
        result = _read_bobbin_metrics(tmp_path, "run1", [])
        assert result is not None
        assert result["gate_skip_count"] == 1
        assert result["gate_skip_details"][0]["query"] == "q1"


class TestExtractCostMetrics:
    def test_extracts_cost_from_claude_output(self):
        agent_result = {
            "result": {
                "type": "result",
                "total_cost_usd": 5.25,
                "usage": {
                    "input_tokens": 10000,
                    "output_tokens": 2000,
                    "cache_read_input_tokens": 5000,
                    "cache_creation_input_tokens": 3000,
                },
                "num_turns": 7,
                "modelUsage": {
                    "claude-opus-4-6": {
                        "inputTokens": 10000,
                        "outputTokens": 2000,
                        "costUSD": 5.25,
                    }
                },
            },
            "output_raw": "",
            "exit_code": 0,
        }
        metrics = _extract_cost_metrics(agent_result)
        assert metrics["cost_usd"] == 5.25
        assert metrics["input_tokens"] == 18000  # 10000 + 5000 + 3000
        assert metrics["output_tokens"] == 2000
        assert metrics["num_turns"] == 7
        assert "model_usage" in metrics

    def test_returns_empty_when_no_result(self):
        agent_result = {"result": None, "output_raw": "", "exit_code": 1}
        metrics = _extract_cost_metrics(agent_result)
        assert metrics == {}

    def test_returns_empty_when_result_is_raw_text(self):
        agent_result = {"result": {"raw_text": "not json"}, "exit_code": 0}
        metrics = _extract_cost_metrics(agent_result)
        # raw_text result has no cost data
        assert "cost_usd" not in metrics

    def test_handles_missing_usage_block(self):
        agent_result = {
            "result": {"total_cost_usd": 1.50},
            "exit_code": 0,
        }
        metrics = _extract_cost_metrics(agent_result)
        assert metrics["cost_usd"] == 1.50
        assert "input_tokens" not in metrics or metrics["input_tokens"] == 0

    def test_handles_partial_usage(self):
        agent_result = {
            "result": {
                "total_cost_usd": 2.00,
                "usage": {"input_tokens": 500, "output_tokens": 100},
            },
            "exit_code": 0,
        }
        metrics = _extract_cost_metrics(agent_result)
        assert metrics["cost_usd"] == 2.00
        assert metrics["input_tokens"] == 500
        assert metrics["output_tokens"] == 100


class TestExtractOutputSummary:
    def test_extracts_result_text(self):
        agent_result = {
            "result": {"result": "I fixed the bug by updating the parser."},
            "exit_code": 0,
        }
        summary = _extract_output_summary(agent_result)
        assert summary == "I fixed the bug by updating the parser."

    def test_truncates_long_output(self):
        long_text = "x" * 5000
        agent_result = {
            "result": {"result": long_text},
            "exit_code": 0,
        }
        summary = _extract_output_summary(agent_result)
        assert len(summary) == 2000

    def test_returns_none_when_no_result(self):
        agent_result = {"result": None, "exit_code": 1}
        assert _extract_output_summary(agent_result) is None

    def test_returns_none_when_empty_text(self):
        agent_result = {"result": {"result": ""}, "exit_code": 0}
        assert _extract_output_summary(agent_result) is None

    def test_returns_none_when_no_result_key(self):
        agent_result = {"result": {"total_cost_usd": 1.0}, "exit_code": 0}
        assert _extract_output_summary(agent_result) is None


class TestSaveRawStream:
    def test_saves_stream_file(self, tmp_path):
        raw = '{"type":"assistant"}\n{"type":"result"}\n'
        dest = _save_raw_stream(
            raw, tmp_path, task_id="ruff-001", approach="no-bobbin", attempt=0,
        )
        assert dest is not None
        assert dest.name == "ruff-001_no-bobbin_0.stream.jsonl"
        assert dest.read_text(encoding="utf-8") == raw

    def test_saves_into_run_dir(self, tmp_path):
        raw = '{"type":"result"}\n'
        dest = _save_raw_stream(
            raw, tmp_path, task_id="ruff-001", approach="with-bobbin", attempt=1,
            run_id="20260101-120000-abcd",
        )
        assert dest is not None
        assert "runs/20260101-120000-abcd" in str(dest)
        assert dest.name == "ruff-001_with-bobbin_1.stream.jsonl"

    def test_returns_none_for_empty_output(self, tmp_path):
        dest = _save_raw_stream(
            "", tmp_path, task_id="ruff-001", approach="no-bobbin", attempt=0,
        )
        assert dest is None

    def test_never_raises_on_failure(self, tmp_path):
        # Pass a read-only path that can't be written to (non-existent nested dir
        # with a file blocking the mkdir).
        blocker = tmp_path / "results"
        blocker.write_text("not a dir")
        dest = _save_raw_stream(
            "data", blocker, task_id="t", approach="a", attempt=0, run_id="r",
        )
        assert dest is None
