"""Tests for Claude Code hook bridge."""

import json
import subprocess
import sys
import time
from io import StringIO
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from tambour.hooks.bridge import (
    detect_failure,
    emit_event,
    infer_issue_id,
    main,
    parse_stdin,
)


class TestParseStdin:
    """Tests for parse_stdin function."""

    def test_valid_json(self):
        """Test parsing valid JSON from stdin."""
        input_data = '{"tool_name": "Read", "session_id": "abc123"}'
        with patch("sys.stdin", StringIO(input_data)):
            result = parse_stdin()
        assert result == {"tool_name": "Read", "session_id": "abc123"}

    def test_empty_input(self):
        """Test handling empty stdin."""
        with patch("sys.stdin", StringIO("")):
            result = parse_stdin()
        assert result is None

    def test_whitespace_only(self):
        """Test handling whitespace-only input."""
        with patch("sys.stdin", StringIO("   \n  ")):
            result = parse_stdin()
        assert result is None

    def test_invalid_json(self):
        """Test handling invalid JSON."""
        with patch("sys.stdin", StringIO("not valid json")):
            result = parse_stdin()
        assert result is None

    def test_partial_json(self):
        """Test handling truncated/partial JSON."""
        with patch("sys.stdin", StringIO('{"tool_name": "Read"')):
            result = parse_stdin()
        assert result is None

    def test_complex_nested_json(self):
        """Test parsing complex nested JSON."""
        input_data = json.dumps(
            {
                "session_id": "sess_123",
                "tool_name": "Read",
                "tool_input": {"file_path": "/path/to/file.rs", "limit": 100},
                "tool_response": {"success": True, "lines": 50},
            }
        )
        with patch("sys.stdin", StringIO(input_data)):
            result = parse_stdin()
        assert result["tool_name"] == "Read"
        assert result["tool_input"]["file_path"] == "/path/to/file.rs"


class TestDetectFailure:
    """Tests for detect_failure function."""

    def test_success_response(self):
        """Test detecting successful response."""
        response = {"success": True, "data": "some result"}
        is_failed, error = detect_failure(response)
        assert is_failed is False
        assert error is None

    def test_explicit_error_field(self):
        """Test detecting failure via error field."""
        response = {"error": "File not found"}
        is_failed, error = detect_failure(response)
        assert is_failed is True
        assert error == "File not found"

    def test_success_false(self):
        """Test detecting failure via success: false."""
        response = {"success": False, "message": "Permission denied"}
        is_failed, error = detect_failure(response)
        assert is_failed is True
        assert error == "Permission denied"

    def test_success_string_false(self):
        """Test detecting failure via success: 'false' string."""
        response = {"success": "false"}
        is_failed, error = detect_failure(response)
        assert is_failed is True

    def test_is_error_field(self):
        """Test detecting failure via is_error field."""
        response = {"is_error": True, "content": "Tool execution failed"}
        is_failed, error = detect_failure(response)
        assert is_failed is True
        assert error == "Tool execution failed"

    def test_no_failure_indicators(self):
        """Test response with no failure indicators."""
        response = {"lines": 100, "content": "file content"}
        is_failed, error = detect_failure(response)
        assert is_failed is False
        assert error is None

    def test_empty_response(self):
        """Test handling empty response dict."""
        is_failed, error = detect_failure({})
        assert is_failed is False
        assert error is None


class TestInferIssueId:
    """Tests for infer_issue_id function."""

    def test_worktree_path(self):
        """Test inferring issue ID from worktree path."""
        cwd = "/home/user/project-worktrees/bobbin-xyz"
        issue_id = infer_issue_id(cwd)
        assert issue_id == "bobbin-xyz"

    def test_worktree_with_subissue(self):
        """Test inferring issue ID with sub-issue notation."""
        cwd = "/home/user/project-worktrees/bobbin-abc.2"
        issue_id = infer_issue_id(cwd)
        assert issue_id == "bobbin-abc.2"

    def test_matching_directory_name(self):
        """Test inferring from directory name matching issue pattern."""
        cwd = "/some/path/proj-123"
        issue_id = infer_issue_id(cwd)
        assert issue_id == "proj-123"

    def test_non_matching_directory(self):
        """Test non-matching directory returns None."""
        cwd = "/home/user/projects/myproject"
        issue_id = infer_issue_id(cwd)
        assert issue_id is None

    def test_main_repo(self):
        """Test main repo (not a worktree) returns None."""
        cwd = "/home/user/projects/bobbin"
        issue_id = infer_issue_id(cwd)
        assert issue_id is None

    def test_complex_issue_id(self):
        """Test complex issue ID patterns."""
        cwd = "/path/to/repo-worktrees/feature-abc123.sub1.sub2"
        issue_id = infer_issue_id(cwd)
        assert issue_id == "feature-abc123.sub1.sub2"


class TestEmitEvent:
    """Tests for emit_event function."""

    @patch("subprocess.run")
    def test_emit_tool_used(self, mock_run):
        """Test emitting tool.used event."""
        mock_run.return_value = MagicMock(returncode=0)

        result = emit_event(
            event_type="tool.used",
            tool_name="Read",
            session_id="sess_123",
            issue_id="bobbin-xyz",
            worktree="/path/to/worktree",
        )

        assert result == 0
        mock_run.assert_called_once()
        cmd = mock_run.call_args[0][0]
        assert "events" in cmd
        assert "emit" in cmd
        assert "tool.used" in cmd
        assert "--issue" in cmd
        assert "bobbin-xyz" in cmd

    @patch("subprocess.run")
    def test_emit_tool_failed(self, mock_run):
        """Test emitting tool.failed event."""
        mock_run.return_value = MagicMock(returncode=0)

        result = emit_event(
            event_type="tool.failed",
            tool_name="Edit",
            session_id="sess_123",
            issue_id=None,
            worktree=None,
            extra_data={"error": "old_string not found"},
        )

        assert result == 0
        cmd = mock_run.call_args[0][0]
        assert "tool.failed" in cmd

    @patch("subprocess.run")
    def test_emit_with_extra_data(self, mock_run):
        """Test emitting event with extra data."""
        mock_run.return_value = MagicMock(returncode=0)

        result = emit_event(
            event_type="tool.used",
            tool_name="Bash",
            session_id="sess_123",
            issue_id=None,
            worktree=None,
            extra_data={"command": "ls -la"},
        )

        assert result == 0
        # Verify data JSON is passed
        cmd = mock_run.call_args[0][0]
        data_idx = cmd.index("--data") + 1
        data_json = json.loads(cmd[data_idx])
        assert data_json["tool_name"] == "Bash"
        assert data_json["command"] == "ls -la"

    @patch("subprocess.run")
    def test_emit_timeout_handling(self, mock_run):
        """Test handling subprocess timeout."""
        mock_run.side_effect = subprocess.TimeoutExpired(cmd=[], timeout=3)

        result = emit_event(
            event_type="tool.used",
            tool_name="Read",
            session_id="sess_123",
            issue_id=None,
            worktree=None,
        )

        assert result == 1

    @patch("subprocess.run")
    def test_emit_exception_handling(self, mock_run):
        """Test handling subprocess exceptions."""
        mock_run.side_effect = OSError("spawn failed")

        result = emit_event(
            event_type="tool.used",
            tool_name="Read",
            session_id="sess_123",
            issue_id=None,
            worktree=None,
        )

        assert result == 1


class TestMain:
    """Tests for main function."""

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_read_tool(self, mock_emit):
        """Test main with Read tool event."""
        mock_emit.return_value = 0

        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_name": "Read",
                "tool_input": {"file_path": "/path/to/file.rs"},
                "tool_response": {"success": True, "lines": 150},
                "cwd": "/path/to/project-worktrees/bobbin-xyz",
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        mock_emit.assert_called_once()
        call_kwargs = mock_emit.call_args
        assert call_kwargs[1]["event_type"] == "tool.used"
        assert call_kwargs[1]["tool_name"] == "Read"
        assert call_kwargs[1]["issue_id"] == "bobbin-xyz"
        assert call_kwargs[1]["extra_data"]["file_path"] == "/path/to/file.rs"

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_failed_tool(self, mock_emit):
        """Test main with failed tool event."""
        mock_emit.return_value = 0

        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_name": "Edit",
                "tool_input": {"file_path": "/path/to/file.rs", "old_string": "foo"},
                "tool_response": {"success": False, "message": "old_string not found"},
                "cwd": "/path/to/project",
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        call_kwargs = mock_emit.call_args
        assert call_kwargs[1]["event_type"] == "tool.failed"
        assert "error" in call_kwargs[1]["extra_data"]

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_bash_tool(self, mock_emit):
        """Test main with Bash tool event."""
        mock_emit.return_value = 0

        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_name": "Bash",
                "tool_input": {"command": "git status"},
                "tool_response": {"success": True},
                "cwd": "/path/to/project",
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        call_kwargs = mock_emit.call_args
        assert call_kwargs[1]["extra_data"]["command"] == "git status"

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_truncates_long_commands(self, mock_emit):
        """Test that long Bash commands are truncated."""
        mock_emit.return_value = 0

        long_command = "echo " + "x" * 300
        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_name": "Bash",
                "tool_input": {"command": long_command},
                "tool_response": {"success": True},
                "cwd": "/path/to/project",
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        call_kwargs = mock_emit.call_args
        assert len(call_kwargs[1]["extra_data"]["command"]) == 200

    def test_main_empty_input(self):
        """Test main with empty input returns success."""
        with patch("sys.stdin", StringIO("")):
            result = main()
        assert result == 0

    def test_main_invalid_json(self):
        """Test main with invalid JSON returns success (graceful)."""
        with patch("sys.stdin", StringIO("not json")):
            result = main()
        assert result == 0

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_missing_tool_name(self, mock_emit):
        """Test main with missing tool_name returns success (graceful)."""
        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_input": {"file_path": "/path/to/file.rs"},
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        mock_emit.assert_not_called()

    @patch("tambour.hooks.bridge.emit_event")
    def test_main_missing_session_id(self, mock_emit):
        """Test main uses 'unknown' for missing session_id."""
        mock_emit.return_value = 0

        input_data = json.dumps(
            {
                "tool_name": "Read",
                "tool_input": {"file_path": "/path/to/file.rs"},
                "tool_response": {"success": True},
            }
        )

        with patch("sys.stdin", StringIO(input_data)):
            result = main()

        assert result == 0
        call_kwargs = mock_emit.call_args
        assert call_kwargs[1]["session_id"] == "unknown"


class TestPerformance:
    """Performance tests for the bridge."""

    def test_parse_stdin_performance(self):
        """Test that parse_stdin is fast."""
        input_data = json.dumps(
            {
                "session_id": "sess_abc123",
                "tool_name": "Read",
                "tool_input": {"file_path": "/path/to/file.rs"},
                "tool_response": {"success": True, "lines": 150, "content": "x" * 1000},
            }
        )

        times = []
        for _ in range(100):
            start = time.perf_counter()
            with patch("sys.stdin", StringIO(input_data)):
                parse_stdin()
            times.append((time.perf_counter() - start) * 1000)

        avg_time = sum(times) / len(times)
        # Should be < 1ms on average
        assert avg_time < 1, f"parse_stdin too slow: {avg_time:.3f}ms"

    def test_detect_failure_performance(self):
        """Test that detect_failure is fast."""
        response = {"success": True, "lines": 150, "content": "x" * 1000}

        times = []
        for _ in range(100):
            start = time.perf_counter()
            detect_failure(response)
            times.append((time.perf_counter() - start) * 1000)

        avg_time = sum(times) / len(times)
        # Should be < 0.1ms on average
        assert avg_time < 0.1, f"detect_failure too slow: {avg_time:.3f}ms"

    def test_infer_issue_id_performance(self):
        """Test that infer_issue_id is fast."""
        cwd = "/home/user/project-worktrees/bobbin-xyz.2"

        times = []
        for _ in range(100):
            start = time.perf_counter()
            infer_issue_id(cwd)
            times.append((time.perf_counter() - start) * 1000)

        avg_time = sum(times) / len(times)
        # Should be < 0.1ms on average
        assert avg_time < 0.1, f"infer_issue_id too slow: {avg_time:.3f}ms"
