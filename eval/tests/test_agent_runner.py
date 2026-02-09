"""Tests for eval.runner.agent_runner module.

Uses mocks for subprocess and shutil.which since we don't invoke real claude CLI.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from runner.agent_runner import AgentRunnerError, _find_claude, run_agent


class TestFindClaude:
    def test_found_on_path(self):
        with patch("runner.agent_runner.shutil.which", return_value="/usr/bin/claude"):
            assert _find_claude() == "/usr/bin/claude"

    def test_found_in_local_bin(self, tmp_path: Path):
        fake_bin = tmp_path / ".local" / "bin" / "claude"
        fake_bin.parent.mkdir(parents=True)
        fake_bin.touch()

        with (
            patch("runner.agent_runner.shutil.which", return_value=None),
            patch("runner.agent_runner.Path.home", return_value=tmp_path),
        ):
            assert _find_claude() == str(fake_bin)

    def test_not_found_raises(self, tmp_path: Path):
        with (
            patch("runner.agent_runner.shutil.which", return_value=None),
            patch("runner.agent_runner.Path.home", return_value=tmp_path),
        ):
            with pytest.raises(AgentRunnerError, match="claude CLI not found"):
                _find_claude()


class TestRunAgent:
    @pytest.fixture()
    def mock_claude(self):
        """Patch _find_claude and subprocess.run for all tests in this class."""
        with (
            patch("runner.agent_runner._find_claude", return_value="/usr/bin/claude"),
            patch("runner.agent_runner.subprocess.run") as mock_run,
        ):
            yield mock_run

    def test_basic_invocation(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps({"result": "hello", "cost_usd": 0.05}),
            stderr="",
        )

        result = run_agent(str(tmp_path), "Fix the bug")

        assert result["exit_code"] == 0
        assert result["timed_out"] is False
        assert result["result"]["result"] == "hello"
        assert result["duration_seconds"] >= 0

        # Verify the command was constructed correctly.
        call_args = mock_claude.call_args
        cmd = call_args[0][0]
        assert cmd[0] == "/usr/bin/claude"
        assert "-p" in cmd
        assert "Fix the bug" in cmd
        assert "--output-format" in cmd
        assert "json" in cmd

    def test_with_settings_file(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="{}", stderr="",
        )

        run_agent(str(tmp_path), "Fix it", settings_file="/path/to/settings.json")

        cmd = mock_claude.call_args[0][0]
        assert "--settings" in cmd
        settings_idx = cmd.index("--settings")
        assert cmd[settings_idx + 1] == "/path/to/settings.json"

    def test_without_settings_file(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="{}", stderr="",
        )

        run_agent(str(tmp_path), "Fix it", settings_file=None)

        cmd = mock_claude.call_args[0][0]
        assert "--settings" not in cmd

    def test_custom_model_and_budget(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="{}", stderr="",
        )

        run_agent(
            str(tmp_path), "task",
            model="claude-opus-4-6",
            max_budget_usd=10.0,
        )

        cmd = mock_claude.call_args[0][0]
        model_idx = cmd.index("--model")
        assert cmd[model_idx + 1] == "claude-opus-4-6"
        budget_idx = cmd.index("--max-budget-usd")
        assert cmd[budget_idx + 1] == "10.0"

    def test_timeout_handling(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.side_effect = subprocess.TimeoutExpired(
            cmd=["claude"], timeout=5, output="partial", stderr="err",
        )

        result = run_agent(str(tmp_path), "slow task", timeout=5)

        assert result["timed_out"] is True
        assert result["exit_code"] == -1

    def test_nonzero_exit_code(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="Error occurred",
        )

        result = run_agent(str(tmp_path), "broken task")

        assert result["exit_code"] == 1
        assert result["result"] is None
        assert "Error occurred" in result["stderr"]

    def test_malformed_json_output(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="not valid json {{{", stderr="",
        )

        result = run_agent(str(tmp_path), "task")

        assert result["result"]["raw_text"] == "not valid json {{{"

    def test_permission_mode_in_command(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="{}", stderr="",
        )

        run_agent(str(tmp_path), "task", permission_mode="bypassPermissions")

        cmd = mock_claude.call_args[0][0]
        assert "--permission-mode" in cmd
        pm_idx = cmd.index("--permission-mode")
        assert cmd[pm_idx + 1] == "bypassPermissions"

    def test_workspace_used_as_cwd(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="{}", stderr="",
        )

        run_agent(str(tmp_path), "task")

        call_kwargs = mock_claude.call_args[1]
        assert call_kwargs["cwd"] == tmp_path
