"""Tests for eval.runner.agent_runner module.

Uses mocks for subprocess and shutil.which since we don't invoke real claude CLI.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from runner.agent_runner import (
    AgentRunnerError,
    _find_claude,
    parse_stream_json,
    run_agent,
)


def _make_stream(*lines: dict) -> str:
    """Build JSONL stdout from a sequence of dicts."""
    return "\n".join(json.dumps(obj) for obj in lines) + "\n"


def _assistant_msg(content: list[dict], turn_idx: int = 0) -> dict:
    """Build a type:assistant stream line."""
    return {
        "type": "assistant",
        "message": {
            "model": "claude-sonnet-4-5-20250929",
            "id": f"msg_{turn_idx}",
            "role": "assistant",
            "content": content,
        },
        "session_id": "test-session",
    }


def _result_line(**overrides) -> dict:
    """Build a type:result stream line."""
    base = {
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "duration_ms": 1000,
        "num_turns": 1,
        "result": "hello",
        "session_id": "test-session",
        "total_cost_usd": 0.05,
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0,
        },
    }
    base.update(overrides)
    return base


# ---------------------------------------------------------------------------
# TestParseStreamJson
# ---------------------------------------------------------------------------


class TestParseStreamJson:
    def test_empty_input(self):
        result = parse_stream_json("")
        assert result["result_line"] is None
        assert result["tool_use_summary"]["by_tool"] == {}
        assert result["tool_use_summary"]["tool_sequence"] == []
        assert result["tool_use_summary"]["first_edit_turn"] is None
        assert result["tool_use_summary"]["bobbin_commands"] == []

    def test_result_line_extraction(self):
        rl = _result_line(total_cost_usd=0.12, result="done")
        raw = _make_stream(rl)
        result = parse_stream_json(raw)
        assert result["result_line"]["total_cost_usd"] == 0.12
        assert result["result_line"]["result"] == "done"

    def test_tool_use_counting(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {"file_path": "/a"}},
                {"type": "tool_use", "name": "Edit", "input": {}},
            ], turn_idx=0),
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {"file_path": "/b"}},
                {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
            ], turn_idx=1),
            _result_line(),
        )
        result = parse_stream_json(stream)
        summary = result["tool_use_summary"]
        assert summary["by_tool"] == {"Read": 2, "Edit": 1, "Bash": 1}
        assert summary["tool_sequence"] == ["Read", "Edit", "Read", "Bash"]

    def test_first_edit_turn(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {}},
            ], turn_idx=0),
            _assistant_msg([
                {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
            ], turn_idx=1),
            _assistant_msg([
                {"type": "tool_use", "name": "Edit", "input": {}},
            ], turn_idx=2),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["first_edit_turn"] == 2

    def test_first_edit_turn_with_write(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Write", "input": {}},
            ], turn_idx=0),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["first_edit_turn"] == 0

    def test_no_edit_means_none(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {}},
                {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
            ]),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["first_edit_turn"] is None

    def test_bobbin_commands(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Bash", "input": {"command": "bobbin search foo"}},
                {"type": "tool_use", "name": "Bash", "input": {"command": "git status"}},
                {"type": "tool_use", "name": "Bash", "input": {"command": "bobbin index --incremental"}},
            ]),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["bobbin_commands"] == [
            "bobbin search foo",
            "bobbin index --incremental",
        ]

    def test_ignores_non_tool_use_content(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "text", "text": "Let me help you."},
                {"type": "tool_use", "name": "Read", "input": {}},
            ]),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["by_tool"] == {"Read": 1}

    def test_ignores_system_lines(self):
        stream = _make_stream(
            {"type": "system", "subtype": "init", "session_id": "s1"},
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {}},
            ]),
            _result_line(),
        )
        result = parse_stream_json(stream)
        assert result["tool_use_summary"]["by_tool"] == {"Read": 1}

    def test_malformed_lines_skipped(self):
        raw = "not json at all\n" + _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {}},
            ]),
            _result_line(),
        )
        result = parse_stream_json(raw)
        assert result["tool_use_summary"]["by_tool"] == {"Read": 1}
        assert result["result_line"] is not None

    def test_no_result_line(self):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {}},
            ]),
        )
        result = parse_stream_json(stream)
        assert result["result_line"] is None
        assert result["tool_use_summary"]["by_tool"] == {"Read": 1}


# ---------------------------------------------------------------------------
# TestFindClaude
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# TestRunAgent
# ---------------------------------------------------------------------------


class TestRunAgent:
    @pytest.fixture()
    def mock_claude(self):
        """Patch _find_claude and subprocess.run for all tests in this class."""
        with (
            patch("runner.agent_runner._find_claude", return_value="/usr/bin/claude"),
            patch("runner.agent_runner.subprocess.run") as mock_run,
        ):
            yield mock_run

    def _stream_stdout(self, **result_overrides) -> str:
        """Build minimal stream-json stdout with a result line."""
        return _make_stream(_result_line(**result_overrides))

    def test_basic_invocation(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=self._stream_stdout(result="hello", total_cost_usd=0.05),
            stderr="",
        )

        result = run_agent(str(tmp_path), "Fix the bug")

        assert result["exit_code"] == 0
        assert result["timed_out"] is False
        assert result["result"]["result"] == "hello"
        assert result["result"]["total_cost_usd"] == 0.05
        assert result["duration_seconds"] >= 0
        assert "tool_use_summary" in result

        # Verify the command was constructed correctly.
        call_args = mock_claude.call_args
        cmd = call_args[0][0]
        assert cmd[0] == "/usr/bin/claude"
        assert "-p" in cmd
        assert "Fix the bug" in cmd
        assert "--output-format" in cmd
        assert "stream-json" in cmd
        assert "--verbose" in cmd

    def test_with_settings_file(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=self._stream_stdout(), stderr="",
        )

        run_agent(str(tmp_path), "Fix it", settings_file="/path/to/settings.json")

        cmd = mock_claude.call_args[0][0]
        assert "--settings" in cmd
        settings_idx = cmd.index("--settings")
        assert cmd[settings_idx + 1] == "/path/to/settings.json"

    def test_without_settings_file(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=self._stream_stdout(), stderr="",
        )

        run_agent(str(tmp_path), "Fix it", settings_file=None)

        cmd = mock_claude.call_args[0][0]
        assert "--settings" not in cmd

    def test_custom_model_and_budget(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=self._stream_stdout(), stderr="",
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

    def test_malformed_output_fallback(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="not valid json {{{", stderr="",
        )

        result = run_agent(str(tmp_path), "task")

        assert result["result"]["raw_text"] == "not valid json {{{"

    def test_json_fallback_when_no_result_line(self, mock_claude: MagicMock, tmp_path: Path):
        """When stream has no type:result line, fall back to json.loads."""
        single_json = json.dumps({"result": "fallback", "total_cost_usd": 0.01})
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=single_json, stderr="",
        )

        result = run_agent(str(tmp_path), "task")

        assert result["result"]["result"] == "fallback"
        assert result["result"]["total_cost_usd"] == 0.01

    def test_tool_use_summary_in_result(self, mock_claude: MagicMock, tmp_path: Path):
        stream = _make_stream(
            _assistant_msg([
                {"type": "tool_use", "name": "Read", "input": {"file_path": "/a"}},
                {"type": "tool_use", "name": "Edit", "input": {}},
            ]),
            _assistant_msg([
                {"type": "tool_use", "name": "Bash", "input": {"command": "bobbin status"}},
            ]),
            _result_line(),
        )
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=stream, stderr="",
        )

        result = run_agent(str(tmp_path), "task")

        summary = result["tool_use_summary"]
        assert summary["by_tool"] == {"Read": 1, "Edit": 1, "Bash": 1}
        assert summary["tool_sequence"] == ["Read", "Edit", "Bash"]
        assert summary["first_edit_turn"] == 0
        assert summary["bobbin_commands"] == ["bobbin status"]

    def test_permission_mode_in_command(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=self._stream_stdout(), stderr="",
        )

        run_agent(str(tmp_path), "task", permission_mode="bypassPermissions")

        cmd = mock_claude.call_args[0][0]
        assert "--permission-mode" in cmd
        pm_idx = cmd.index("--permission-mode")
        assert cmd[pm_idx + 1] == "bypassPermissions"

    def test_workspace_used_as_cwd(self, mock_claude: MagicMock, tmp_path: Path):
        mock_claude.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=self._stream_stdout(), stderr="",
        )

        run_agent(str(tmp_path), "task")

        call_kwargs = mock_claude.call_args[1]
        assert call_kwargs["cwd"] == tmp_path
