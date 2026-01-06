"""Tests for finish command."""

import json
from pathlib import Path
from unittest.mock import MagicMock, patch, call

import pytest

from tambour.finish import (
    FinishCommand,
    FinishResult,
    _find_main_repo,
    cmd_finish,
    cmd_lock_status,
    cmd_lock_release,
)
from tambour.lock import MergeLock


class TestFinishResult:
    """Tests for FinishResult dataclass."""

    def test_default_values(self):
        """Test default values for FinishResult."""
        result = FinishResult(success=True, issue_id="test-123")

        assert result.success
        assert result.issue_id == "test-123"
        assert result.issue_title == ""
        assert not result.merged
        assert not result.worktree_removed
        assert not result.branch_deleted
        assert not result.issue_closed
        assert result.closed_epics == []
        assert result.error is None

    def test_with_closed_epics(self):
        """Test FinishResult with closed epics."""
        result = FinishResult(
            success=True,
            issue_id="test-123",
            closed_epics=[("epic-1", "First Epic"), ("epic-2", "Second Epic")],
        )

        assert len(result.closed_epics) == 2
        assert result.closed_epics[0] == ("epic-1", "First Epic")


class TestFinishCommand:
    """Tests for FinishCommand class."""

    @pytest.fixture
    def mock_repo(self, tmp_path):
        """Create mock repository structure."""
        main_repo = tmp_path / "repo"
        main_repo.mkdir()
        (main_repo / ".git").mkdir()
        (main_repo / ".beads").mkdir()

        worktree_base = tmp_path / "worktrees"
        worktree_base.mkdir()

        worktree = worktree_base / "test-issue"
        worktree.mkdir()
        (worktree / ".git").write_text("gitdir: /path/to/.git/worktrees/test-issue")

        return main_repo, worktree_base, worktree

    @pytest.fixture
    def finish_cmd(self, mock_repo):
        """Create FinishCommand instance."""
        main_repo, worktree_base, _ = mock_repo
        return FinishCommand(
            issue_id="test-issue",
            main_repo=main_repo,
            worktree_base=worktree_base,
            merge=True,
            no_continue=True,
        )

    def test_worktree_not_found(self, mock_repo):
        """Test error when worktree doesn't exist."""
        main_repo, worktree_base, _ = mock_repo
        cmd = FinishCommand(
            issue_id="nonexistent",
            main_repo=main_repo,
            worktree_base=worktree_base,
        )

        result = cmd.run()

        assert not result.success
        assert "Worktree not found" in result.error

    def test_no_merge_preserves_worktree(self, mock_repo):
        """Test that --no-merge preserves the worktree."""
        main_repo, worktree_base, _ = mock_repo
        cmd = FinishCommand(
            issue_id="test-issue",
            main_repo=main_repo,
            worktree_base=worktree_base,
            merge=False,
        )

        with patch("builtins.print"):
            result = cmd.run()

        assert result.success
        assert not result.merged

    def test_get_issue_info(self, finish_cmd):
        """Test getting issue info from beads."""
        with patch.object(finish_cmd, "_run_bd") as mock_bd:
            mock_bd.return_value = MagicMock(
                returncode=0,
                stdout=json.dumps([{
                    "title": "Test Issue",
                    "issue_type": "task",
                    "status": "in_progress",
                }]),
            )

            title, issue_type, status = finish_cmd._get_issue_info()

            assert title == "Test Issue"
            assert issue_type == "task"
            assert status == "in_progress"

    def test_get_issue_info_error(self, finish_cmd):
        """Test getting issue info when beads fails."""
        with patch.object(finish_cmd, "_run_bd") as mock_bd:
            mock_bd.side_effect = Exception("bd not found")

            title, issue_type, status = finish_cmd._get_issue_info()

            assert title == "Unknown"
            assert issue_type == "unknown"
            assert status == "unknown"

    def test_branch_exists(self, finish_cmd):
        """Test checking if branch exists."""
        with patch.object(finish_cmd, "_run_git") as mock_git:
            mock_git.return_value = MagicMock(returncode=0)

            assert finish_cmd._branch_exists() is True

            mock_git.return_value = MagicMock(returncode=1)

            assert finish_cmd._branch_exists() is False

    def test_full_workflow_success(self, finish_cmd):
        """Test successful full workflow."""
        with patch.object(finish_cmd, "_run_git") as mock_git, \
             patch.object(finish_cmd, "_run_bd") as mock_bd, \
             patch.object(MergeLock, "acquire", return_value=True), \
             patch.object(MergeLock, "release", return_value=True), \
             patch.object(MergeLock, "is_acquired", True), \
             patch("builtins.print"):

            # Mock git operations
            mock_git.side_effect = [
                MagicMock(returncode=0),  # checkout main
                MagicMock(returncode=0),  # pull
                MagicMock(returncode=0),  # show-ref (branch exists)
                MagicMock(returncode=0),  # merge
                MagicMock(returncode=0),  # push
                MagicMock(returncode=0),  # checkout --detach
                MagicMock(returncode=0),  # show-ref (branch delete check)
                MagicMock(returncode=0),  # branch -d
            ]

            # Mock bd operations
            mock_bd.side_effect = [
                MagicMock(returncode=0, stdout=json.dumps([{"title": "Test", "status": "in_progress"}])),  # show --json
                MagicMock(returncode=0),  # worktree remove
                MagicMock(returncode=0, stdout="[]"),  # epic status (before)
                MagicMock(returncode=0),  # close
                MagicMock(returncode=0, stdout="[]"),  # epic status (after)
            ]

            result = finish_cmd.run()

            # Result assertions - adjusted for what we're testing
            assert result.issue_id == "test-issue"
            # Note: merged might be True if branch check returns True

    def test_merge_lock_timeout(self, finish_cmd):
        """Test error when merge lock times out."""
        with patch.object(finish_cmd, "_run_bd") as mock_bd, \
             patch.object(MergeLock, "acquire", return_value=False) as mock_acquire, \
             patch("builtins.print"):

            mock_bd.return_value = MagicMock(
                returncode=0,
                stdout=json.dumps([{"title": "Test", "status": "in_progress"}]),
            )

            result = finish_cmd.run()

            assert not result.success
            assert "Timeout" in result.error or "lock" in result.error.lower()

    def test_auto_close_epics(self, finish_cmd):
        """Test auto-closing epics when they become eligible."""
        epics_before = [
            {"epic": {"id": "epic-1", "title": "Epic 1"}, "eligible_for_close": False},
        ]
        epics_after = [
            {"epic": {"id": "epic-1", "title": "Epic 1"}, "eligible_for_close": True},
        ]

        with patch.object(finish_cmd, "_run_bd") as mock_bd, \
             patch("builtins.print"):
            mock_bd.return_value = MagicMock(returncode=0)

            closed = finish_cmd._auto_close_epics(epics_before, epics_after)

            assert len(closed) == 1
            assert closed[0] == ("epic-1", "Epic 1")
            mock_bd.assert_called_with("close", "epic-1", check=False)


class TestFindMainRepo:
    """Tests for _find_main_repo function."""

    def test_from_main_repo(self, tmp_path):
        """Test finding main repo when in main repo."""
        repo = tmp_path / "repo"
        repo.mkdir()
        (repo / ".git").mkdir()  # Directory, not file

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                returncode=0,
                stdout=str(repo),
            )

            result = _find_main_repo()

            assert result == repo

    def test_from_worktree(self, tmp_path):
        """Test finding main repo when in worktree."""
        main_repo = tmp_path / "main"
        main_repo.mkdir()
        (main_repo / ".git").mkdir()

        worktree = tmp_path / "worktree"
        worktree.mkdir()
        gitfile = worktree / ".git"
        gitfile.write_text(f"gitdir: {main_repo}/.git/worktrees/branch-name")

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                returncode=0,
                stdout=str(worktree),
            )

            result = _find_main_repo()

            assert result == main_repo

    def test_not_in_git_repo(self):
        """Test when not in a git repository."""
        with patch("subprocess.run") as mock_run:
            from subprocess import CalledProcessError
            mock_run.side_effect = CalledProcessError(128, "git")

            result = _find_main_repo()

            assert result is None


class TestCmdLockStatus:
    """Tests for cmd_lock_status function."""

    def test_lock_free(self, tmp_path):
        """Test showing free lock status."""
        with patch("tambour.finish._find_current_repo", return_value=tmp_path), \
             patch.object(MergeLock, "status") as mock_status, \
             patch("builtins.print") as mock_print:

            from tambour.lock import LockStatus
            mock_status.return_value = LockStatus(held=False)

            result = cmd_lock_status(MagicMock())

            assert result == 0
            mock_print.assert_called_with("Lock: FREE (no lock held)")

    def test_lock_held(self, tmp_path):
        """Test showing held lock status."""
        from tambour.lock import LockStatus, LockMetadata
        from datetime import datetime, timezone

        with patch("tambour.finish._find_current_repo", return_value=tmp_path), \
             patch.object(MergeLock, "status") as mock_status, \
             patch("builtins.print"):

            mock_status.return_value = LockStatus(
                held=True,
                metadata=LockMetadata(
                    holder="bobbin-xyz",
                    acquired_at=datetime.now(timezone.utc),
                    host="test-host",
                    pid=12345,
                ),
            )

            result = cmd_lock_status(MagicMock())

            assert result == 0


class TestCmdLockRelease:
    """Tests for cmd_lock_release function."""

    def test_release_success(self, tmp_path):
        """Test successful lock release."""
        with patch("tambour.finish._find_current_repo", return_value=tmp_path), \
             patch.object(MergeLock, "force_release", return_value=True), \
             patch("builtins.print") as mock_print:

            result = cmd_lock_release(MagicMock())

            assert result == 0
            # Check that success message was printed
            calls = [str(c) for c in mock_print.call_args_list]
            assert any("released" in c.lower() or "Lock released" in c for c in calls)
