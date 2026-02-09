"""Tests for eval.scorer.test_scorer module.

Uses mocks for subprocess since we don't run real test suites.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import patch

import pytest

from scorer.test_scorer import (
    _parse_cargo_test_output,
    _parse_pytest_output,
    run_tests,
)


class TestParsePytestOutput:
    def test_all_passed(self):
        output = "===== 10 passed in 1.23s ====="
        result = _parse_pytest_output(output)
        assert result["framework"] == "pytest"
        assert result["passed"] == 10
        assert result["failed"] == 0
        assert result["total"] == 10

    def test_mixed_results(self):
        output = "===== 5 passed, 2 failed, 1 error in 3.45s ====="
        result = _parse_pytest_output(output)
        assert result["passed"] == 5
        assert result["failed"] == 3  # 2 failed + 1 error
        assert result["total"] == 8

    def test_with_skipped(self):
        output = "===== 8 passed, 1 failed, 2 skipped in 2.00s ====="
        result = _parse_pytest_output(output)
        assert result["passed"] == 8
        assert result["failed"] == 1
        assert result["skipped"] == 2
        assert result["total"] == 11

    def test_no_match(self):
        output = "some random output with no pytest summary"
        result = _parse_pytest_output(output)
        assert result == {}

    def test_errors_counted_as_failures(self):
        output = "===== 3 passed, 2 errored in 1.00s ====="
        result = _parse_pytest_output(output)
        assert result["passed"] == 3
        assert result["failed"] == 2

    def test_only_failed(self):
        output = "===== 4 failed in 0.50s ====="
        result = _parse_pytest_output(output)
        assert result["failed"] == 4
        assert result["passed"] == 0


class TestParseCargoTestOutput:
    def test_all_passed(self):
        output = "test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out"
        result = _parse_cargo_test_output(output)
        assert result["framework"] == "cargo-test"
        assert result["passed"] == 42
        assert result["failed"] == 0
        assert result["total"] == 42

    def test_with_failures(self):
        output = "test result: FAILED. 10 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out"
        result = _parse_cargo_test_output(output)
        assert result["passed"] == 10
        assert result["failed"] == 3
        assert result["skipped"] == 1
        assert result["total"] == 14

    def test_multiple_result_lines(self):
        output = (
            "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
            "\n"
            "test result: ok. 3 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out\n"
        )
        result = _parse_cargo_test_output(output)
        assert result["passed"] == 8
        assert result["failed"] == 1
        assert result["skipped"] == 2
        assert result["total"] == 11

    def test_no_match(self):
        output = "compiling some_crate v0.1.0"
        result = _parse_cargo_test_output(output)
        assert result == {}


class TestRunTests:
    @pytest.fixture()
    def mock_subprocess(self):
        with patch("scorer.test_scorer.subprocess.run") as mock_run:
            yield mock_run

    def test_passing_tests(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="===== 5 passed in 1.00s =====",
            stderr="",
        )

        result = run_tests(str(tmp_path), "pytest tests/")

        assert result["passed"] is True
        assert result["total"] == 5
        assert result["failures"] == 0
        assert result["exit_code"] == 0
        assert result["timed_out"] is False
        assert result["parsed"]["framework"] == "pytest"

    def test_failing_tests(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="===== 3 passed, 2 failed in 1.00s =====",
            stderr="",
        )

        result = run_tests(str(tmp_path), "pytest tests/")

        assert result["passed"] is False
        assert result["total"] == 5
        assert result["failures"] == 2
        assert result["exit_code"] == 1

    def test_timeout(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.side_effect = subprocess.TimeoutExpired(
            cmd=["sh", "-c", "pytest"], timeout=10, output="partial output", stderr="",
        )

        result = run_tests(str(tmp_path), "pytest tests/", timeout=10)

        assert result["passed"] is False
        assert result["timed_out"] is True
        assert result["exit_code"] == -1

    def test_unparseable_output_passing(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="All good, no parseable summary",
            stderr="",
        )

        result = run_tests(str(tmp_path), "custom_test_runner")

        assert result["passed"] is True
        assert result["total"] == 0
        assert result["failures"] == 0
        assert result["parsed"] == {}

    def test_unparseable_output_failing(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="something broke",
            stderr="error details",
        )

        result = run_tests(str(tmp_path), "custom_test_runner")

        assert result["passed"] is False
        assert result["failures"] == -1  # sentinel for unparseable
        assert result["parsed"] == {}

    def test_cargo_test_detection(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            stderr="",
        )

        result = run_tests(str(tmp_path), "cargo test")

        assert result["parsed"]["framework"] == "cargo-test"
        assert result["total"] == 20

    def test_command_runs_in_workspace(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        run_tests(str(tmp_path), "pytest tests/")

        call_kwargs = mock_subprocess.call_args[1]
        assert call_kwargs["cwd"] == tmp_path

    def test_output_combines_stdout_and_stderr(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="stdout content\n",
            stderr="stderr content\n",
        )

        result = run_tests(str(tmp_path), "pytest tests/")

        assert "stdout content" in result["output"]
        assert "stderr content" in result["output"]

    def test_custom_timeout(self, mock_subprocess, tmp_path: Path):
        mock_subprocess.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="", stderr="",
        )

        run_tests(str(tmp_path), "pytest", timeout=120)

        call_kwargs = mock_subprocess.call_args[1]
        assert call_kwargs["timeout"] == 120
