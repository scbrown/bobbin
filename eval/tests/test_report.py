"""Tests for analysis.report."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from analysis.report import ReportError, generate_report


def _make_result(
    task_id: str = "ruff-001",
    approach: str = "no-bobbin",
    attempt: int = 0,
    passed: bool = True,
    precision: float = 0.75,
    recall: float = 0.80,
    f1: float = 0.77,
    duration: float = 120.5,
    cost_usd: float | None = None,
    input_tokens: int | None = None,
    output_tokens: int | None = None,
    tool_use_summary: dict | None = None,
) -> dict:
    """Build a minimal result dict matching the expected schema."""
    agent_result: dict = {
        "exit_code": 0 if passed else 1,
        "duration_seconds": duration,
        "timed_out": False,
    }
    if cost_usd is not None:
        agent_result["cost_usd"] = cost_usd
    if input_tokens is not None:
        agent_result["input_tokens"] = input_tokens
    if output_tokens is not None:
        agent_result["output_tokens"] = output_tokens

    result = {
        "task_id": task_id,
        "approach": approach,
        "attempt": attempt,
        "status": "completed",
        "agent_result": agent_result,
        "test_result": {
            "passed": passed,
            "total": 10,
            "failures": 0 if passed else 3,
            "parsed": {"framework": "cargo-test", "passed": 10, "failed": 0},
        },
        "diff_result": {
            "file_precision": precision,
            "file_recall": recall,
            "f1": f1,
            "files_touched": ["src/a.rs"],
            "ground_truth_files": ["src/a.rs", "src/b.rs"],
            "exact_file_match": False,
        },
    }

    if cost_usd is not None or input_tokens is not None or output_tokens is not None:
        result["token_usage"] = {
            "total_cost_usd": cost_usd,
            "input_tokens": input_tokens or 0,
            "output_tokens": output_tokens or 0,
        }

    if tool_use_summary is not None:
        result["tool_use_summary"] = tool_use_summary

    return result


def _write_results(results_dir: Path, results: list[dict]) -> None:
    """Write a list of result dicts as JSON files."""
    results_dir.mkdir(parents=True, exist_ok=True)
    for i, r in enumerate(results):
        tid = r.get("task_id", "unknown")
        approach = r.get("approach", "unknown")
        attempt = r.get("attempt", i)
        path = results_dir / f"{tid}_{approach}_{attempt}.json"
        path.write_text(json.dumps(r), encoding="utf-8")


class TestGenerateReport:
    def test_basic_report(self, tmp_path):
        results = [
            _make_result(approach="no-bobbin", attempt=0, passed=False, precision=0.5),
            _make_result(approach="no-bobbin", attempt=1, passed=True, precision=0.6),
            _make_result(approach="with-bobbin", attempt=0, passed=True, precision=0.9),
            _make_result(approach="with-bobbin", attempt=1, passed=True, precision=0.85),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        assert output.exists()
        content = output.read_text()
        assert "# Bobbin Eval Report" in content
        assert "no-bobbin" in content
        assert "with-bobbin" in content
        assert "Summary" in content
        assert "Per-Task Breakdown" in content

    def test_report_with_single_approach(self, tmp_path):
        results = [
            _make_result(approach="no-bobbin", attempt=0),
            _make_result(approach="no-bobbin", attempt=1),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "no-bobbin" in content
        # No Delta column with single approach.
        assert "Delta" not in content

    def test_report_with_multiple_tasks(self, tmp_path):
        results = [
            _make_result(task_id="ruff-001", approach="no-bobbin"),
            _make_result(task_id="ruff-001", approach="with-bobbin"),
            _make_result(task_id="flask-001", approach="no-bobbin"),
            _make_result(task_id="flask-001", approach="with-bobbin"),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "ruff-001" in content
        assert "flask-001" in content

    def test_creates_parent_dirs(self, tmp_path):
        results = [_make_result()]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "nested" / "dir" / "report.md"
        generate_report(str(rdir), str(output))
        assert output.exists()

    def test_no_results_dir(self, tmp_path):
        with pytest.raises(ReportError, match="not found"):
            generate_report(str(tmp_path / "nonexistent"), str(tmp_path / "out.md"))

    def test_empty_results_dir(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        with pytest.raises(ReportError, match="No result JSON"):
            generate_report(str(rdir), str(tmp_path / "out.md"))

    def test_skips_invalid_json(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        # One valid, one invalid.
        (rdir / "good.json").write_text(json.dumps(_make_result()))
        (rdir / "bad.json").write_text("not json{{{")

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))
        assert output.exists()

    def test_all_invalid_json(self, tmp_path):
        rdir = tmp_path / "results"
        rdir.mkdir()
        (rdir / "bad1.json").write_text("nope")
        (rdir / "bad2.json").write_text("{broken")

        with pytest.raises(ReportError, match="No.*result.*JSON.*files.*found"):
            generate_report(str(rdir), str(tmp_path / "out.md"))

    def test_report_metrics_accuracy(self, tmp_path):
        """Verify computed metrics are correct."""
        results = [
            _make_result(approach="no-bobbin", attempt=0, passed=True, precision=0.4, recall=0.6, f1=0.48, duration=100.0),
            _make_result(approach="no-bobbin", attempt=1, passed=False, precision=0.6, recall=0.8, f1=0.69, duration=200.0),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        # Pass rate: 1/2 = 50%
        assert "50.0%" in content
        # Avg precision: (0.4 + 0.6) / 2 = 0.5 → 50.0%
        assert "50.0%" in content


class TestReportDelta:
    def test_delta_column_present(self, tmp_path):
        results = [
            _make_result(approach="no-bobbin", passed=False, precision=0.4),
            _make_result(approach="with-bobbin", passed=True, precision=0.8),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "Delta" in content


class TestReportCostMetrics:
    def test_cost_columns_present_when_data_exists(self, tmp_path):
        """When results have cost data, report should include cost columns."""
        results = [
            _make_result(approach="no-bobbin", attempt=0, cost_usd=5.50, input_tokens=10000, output_tokens=2000),
            _make_result(approach="with-bobbin", attempt=0, cost_usd=3.25, input_tokens=8000, output_tokens=1500),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "Avg Cost (USD)" in content
        assert "Avg Input Tokens" in content
        assert "Avg Output Tokens" in content
        assert "$5.50" in content
        assert "$3.25" in content

    def test_cost_columns_absent_without_data(self, tmp_path):
        """When results lack cost data, report should not include cost columns."""
        results = [
            _make_result(approach="no-bobbin", attempt=0),
            _make_result(approach="with-bobbin", attempt=0),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        assert "Avg Cost (USD)" not in content
        assert "Avg Input Tokens" not in content

    def test_per_task_table_includes_cost(self, tmp_path):
        """Per-task table should include cost column when data exists."""
        results = [
            _make_result(task_id="ruff-001", approach="no-bobbin", cost_usd=4.00),
            _make_result(task_id="ruff-001", approach="with-bobbin", cost_usd=2.50),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        # Per-task table should have Cost header.
        assert "| Cost |" in content


class TestReportToolUseColumns:
    def test_report_includes_tool_use_columns(self, tmp_path):
        """When results have tool_use_summary, report should include tool use rows."""
        tool_summary = {
            "by_tool": {"Bash": 10, "Read": 5, "Edit": 3},
            "total_tool_calls": 18,
            "bobbin_commands": [{"command": "bobbin search auth", "turn": 2}],
            "first_edit_turn": 7,
            "tool_sequence": ["Read", "Grep", "Bash", "Edit"],
        }
        results = [
            _make_result(
                approach="no-bobbin",
                attempt=0,
                tool_use_summary={
                    "by_tool": {"Bash": 8, "Read": 4, "Edit": 2},
                    "total_tool_calls": 14,
                    "bobbin_commands": [],
                    "first_edit_turn": 5,
                    "tool_sequence": ["Read", "Bash", "Edit"],
                },
            ),
            _make_result(
                approach="with-bobbin",
                attempt=0,
                tool_use_summary=tool_summary,
            ),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        # Summary table should have tool use metric rows.
        assert "Avg Tool Calls" in content
        assert "Avg First Edit Turn" in content
        assert "Avg Bobbin Commands" in content
        # Per-task table should have Tools column.
        assert "| Tools |" in content

    def test_report_backward_compat_no_tool_use(self, tmp_path):
        """Results without tool_use_summary should still generate valid reports."""
        results = [
            _make_result(approach="no-bobbin", attempt=0),
            _make_result(approach="with-bobbin", attempt=0),
        ]
        rdir = tmp_path / "results"
        _write_results(rdir, results)

        output = tmp_path / "report.md"
        generate_report(str(rdir), str(output))

        content = output.read_text()
        # Report should still generate without errors.
        assert "# Bobbin Eval Report" in content
        assert "Summary" in content
        # Tool use rows should NOT appear when no data exists.
        assert "Avg Tool Calls" not in content
        assert "Avg First Edit Turn" not in content
        assert "Avg Bobbin Commands" not in content
        # Per-task table should NOT have Tools column.
        assert "| Tools |" not in content
