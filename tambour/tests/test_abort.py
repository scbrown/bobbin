"""Tests for the abort command."""

import subprocess
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from tambour.__main__ import cmd_abort, create_parser


class TestAbortCommandParsing:
    """Tests for abort command argument parsing."""

    def test_abort_parser_exists(self):
        """Test that abort subcommand is registered."""
        parser = create_parser()
        args = parser.parse_args(["abort", "test-issue"])
        assert args.command == "abort"
        assert args.issue == "test-issue"

    def test_abort_with_worktree_base(self):
        """Test abort with custom worktree base."""
        parser = create_parser()
        args = parser.parse_args(
            ["abort", "test-issue", "--worktree-base", "/custom/path"]
        )
        assert args.issue == "test-issue"
        assert args.worktree_base == "/custom/path"

    def test_abort_requires_issue(self):
        """Test that abort requires an issue ID."""
        parser = create_parser()
        with pytest.raises(SystemExit):
            parser.parse_args(["abort"])


class TestAbortCommand:
    """Tests for abort command execution."""

    @patch("subprocess.run")
    def test_abort_unclaims_issue(self, mock_run):
        """Test that abort unclaims the issue."""
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as tmpdir:
            # Create a mock git repo
            git_root = Path(tmpdir) / "repo"
            git_root.mkdir()
            (git_root / ".git").mkdir()

            # Create mock worktree base
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()

            # Create the parser and args
            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Check that bd update was called to unclaim
            calls = [call for call in mock_run.call_args_list]
            bd_update_calls = [
                call for call in calls if call[0][0][:2] == ["bd", "update"]
            ]
            assert len(bd_update_calls) == 1
            assert "--status" in bd_update_calls[0][0][0]
            assert "open" in bd_update_calls[0][0][0]
            assert "--assignee" in bd_update_calls[0][0][0]

    @patch("subprocess.run")
    def test_abort_removes_worktree(self, mock_run):
        """Test that abort removes the worktree."""
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as tmpdir:
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()

            # Create a fake worktree directory
            worktree_path = worktree_base / "test-issue"
            worktree_path.mkdir()
            (worktree_path / "some_file.txt").write_text("content")

            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Check that bd worktree remove was called
            calls = [call for call in mock_run.call_args_list]
            worktree_remove_calls = [
                call
                for call in calls
                if len(call[0][0]) >= 3 and call[0][0][:3] == ["bd", "worktree", "remove"]
            ]
            assert len(worktree_remove_calls) >= 1

    @patch("subprocess.run")
    def test_abort_deletes_branch(self, mock_run):
        """Test that abort deletes the feature branch."""
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as tmpdir:
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()

            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Check that git branch -D was called
            calls = [call for call in mock_run.call_args_list]
            branch_delete_calls = [
                call
                for call in calls
                if len(call[0][0]) >= 3 and call[0][0][:3] == ["git", "branch", "-D"]
            ]
            assert len(branch_delete_calls) == 1
            assert "test-issue" in branch_delete_calls[0][0][0]

    @patch("subprocess.run")
    def test_abort_handles_missing_worktree(self, mock_run):
        """Test that abort handles missing worktree gracefully."""
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as tmpdir:
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()
            # Don't create the worktree directory

            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Should succeed even without worktree
            assert result == 0

    @patch("subprocess.run")
    def test_abort_handles_bd_update_failure(self, mock_run):
        """Test that abort continues even if bd update fails."""

        def side_effect(cmd, **kwargs):
            if cmd[:2] == ["bd", "update"]:
                return MagicMock(returncode=1, stdout="", stderr="Issue not found")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = side_effect

        with tempfile.TemporaryDirectory() as tmpdir:
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()

            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Should still succeed
            assert result == 0

    @patch("subprocess.run")
    def test_abort_handles_branch_not_found(self, mock_run):
        """Test that abort handles missing branch gracefully."""

        def side_effect(cmd, **kwargs):
            if cmd[:3] == ["git", "branch", "-D"]:
                return MagicMock(
                    returncode=1, stdout="", stderr="error: branch 'test-issue' not found"
                )
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = side_effect

        with tempfile.TemporaryDirectory() as tmpdir:
            worktree_base = Path(tmpdir) / "worktrees"
            worktree_base.mkdir()

            parser = create_parser()
            args = parser.parse_args(
                ["abort", "test-issue", "--worktree-base", str(worktree_base)]
            )

            result = cmd_abort(args)

            # Should still succeed
            assert result == 0

    @patch("subprocess.run")
    def test_abort_detects_git_root(self, mock_run):
        """Test that abort finds git root when no worktree-base specified."""
        # Mock git rev-parse to return a path
        def side_effect(cmd, **kwargs):
            if cmd[:3] == ["git", "rev-parse", "--show-toplevel"]:
                return MagicMock(returncode=0, stdout="/path/to/repo\n", stderr="")
            return MagicMock(returncode=0, stdout="", stderr="")

        mock_run.side_effect = side_effect

        parser = create_parser()
        args = parser.parse_args(["abort", "test-issue"])
        # args.worktree_base will be None

        with patch.object(Path, "exists", return_value=False):
            result = cmd_abort(args)

        # Check git rev-parse was called
        calls = [call for call in mock_run.call_args_list]
        rev_parse_calls = [
            call
            for call in calls
            if len(call[0][0]) >= 3 and call[0][0][:3] == ["git", "rev-parse", "--show-toplevel"]
        ]
        assert len(rev_parse_calls) == 1

    @patch("subprocess.run")
    def test_abort_fails_outside_git_repo(self, mock_run):
        """Test that abort fails when not in a git repo."""
        mock_run.side_effect = subprocess.CalledProcessError(
            128, "git", stderr="fatal: not a git repository"
        )

        parser = create_parser()
        args = parser.parse_args(["abort", "test-issue"])
        # args.worktree_base is None, so it will try to find git root

        result = cmd_abort(args)

        assert result == 1
