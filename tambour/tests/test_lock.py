"""Tests for distributed merge lock."""

import json
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import MagicMock, patch, call

import pytest

from tambour.lock import (
    LOCK_REF,
    LockMetadata,
    LockStatus,
    MergeLock,
)


class TestLockMetadata:
    """Tests for LockMetadata dataclass."""

    def test_to_dict(self):
        """Test serialization to dictionary."""
        dt = datetime(2024, 1, 15, 10, 30, 0, tzinfo=timezone.utc)
        metadata = LockMetadata(
            holder="bobbin-xyz",
            acquired_at=dt,
            host="agent-host",
            pid=12345,
        )

        result = metadata.to_dict()

        assert result["holder"] == "bobbin-xyz"
        assert result["acquired_at"] == "2024-01-15T10:30:00+00:00"
        assert result["host"] == "agent-host"
        assert result["pid"] == 12345

    def test_from_dict(self):
        """Test deserialization from dictionary."""
        data = {
            "holder": "bobbin-abc",
            "acquired_at": "2024-01-15T10:30:00+00:00",
            "host": "test-host",
            "pid": 99999,
        }

        metadata = LockMetadata.from_dict(data)

        assert metadata.holder == "bobbin-abc"
        assert metadata.acquired_at == datetime(2024, 1, 15, 10, 30, 0, tzinfo=timezone.utc)
        assert metadata.host == "test-host"
        assert metadata.pid == 99999


class TestLockStatus:
    """Tests for LockStatus dataclass."""

    def test_not_held(self):
        """Test status when lock is not held."""
        status = LockStatus(held=False)
        assert not status.held
        assert status.holder is None

    def test_held_with_metadata(self):
        """Test status when lock is held with metadata."""
        metadata = LockMetadata(
            holder="bobbin-xyz",
            acquired_at=datetime.now(timezone.utc),
            host="test",
            pid=1234,
        )
        status = LockStatus(held=True, metadata=metadata)
        assert status.held
        assert status.holder == "bobbin-xyz"


class TestMergeLock:
    """Tests for MergeLock class."""

    @pytest.fixture
    def mock_repo(self, tmp_path):
        """Create a mock repo path."""
        return tmp_path / "repo"

    @pytest.fixture
    def lock(self, mock_repo):
        """Create a MergeLock instance with short timeout."""
        return MergeLock(mock_repo, timeout=1)

    def test_status_free(self, lock):
        """Test status when lock is free."""
        with patch.object(lock, "_run_git") as mock_git:
            # fetch returns non-zero when ref doesn't exist
            mock_git.return_value = MagicMock(returncode=128)

            status = lock.status()

            assert not status.held
            mock_git.assert_called_once_with("fetch", "origin", LOCK_REF, check=False)

    def test_status_held_with_metadata(self, lock):
        """Test status when lock is held."""
        lock_data = {
            "holder": "bobbin-xyz",
            "acquired_at": "2024-01-15T10:30:00+00:00",
            "host": "test-host",
            "pid": 12345,
        }

        with patch.object(lock, "_run_git") as mock_git:
            # Mock the git commands
            mock_git.side_effect = [
                MagicMock(returncode=0),  # fetch succeeds
                MagicMock(returncode=0, stdout="commit ..."),  # cat-file
                MagicMock(returncode=0, stdout="100644 blob abc123\tlock.json"),  # ls-tree
                MagicMock(returncode=0, stdout=json.dumps(lock_data)),  # cat-file blob
            ]

            status = lock.status()

            assert status.held
            assert status.holder == "bobbin-xyz"

    def test_acquire_success(self, lock):
        """Test successful lock acquisition."""
        with patch("subprocess.run") as mock_run:
            # Mock successful git operations
            mock_run.side_effect = [
                MagicMock(returncode=0, stdout="abc123"),  # hash-object
                MagicMock(returncode=0, stdout="tree123"),  # mktree
                MagicMock(returncode=0, stdout="commit123"),  # commit-tree
                MagicMock(returncode=0),  # push (success)
            ]

            result = lock.acquire("bobbin-test")

            assert result is True
            assert lock.is_acquired

    def test_acquire_timeout(self, lock):
        """Test lock acquisition timeout."""
        def mock_subprocess(*args, **kwargs):
            """Mock that simulates push always failing."""
            cmd = args[0] if args else kwargs.get("args", [])
            if "hash-object" in cmd:
                return MagicMock(returncode=0, stdout="abc123\n")
            elif "mktree" in cmd:
                return MagicMock(returncode=0, stdout="tree123\n")
            elif "commit-tree" in cmd:
                return MagicMock(returncode=0, stdout="commit123\n")
            elif "push" in cmd:
                return MagicMock(returncode=128, stderr="error: failed to push")
            else:
                return MagicMock(returncode=0)

        with patch("subprocess.run", side_effect=mock_subprocess):
            with patch.object(lock, "_run_git") as mock_status_git:
                mock_status_git.return_value = MagicMock(returncode=128)

                with patch("time.sleep"):  # Speed up test
                    result = lock.acquire("bobbin-test")

            assert result is False
            assert not lock.is_acquired

    def test_release_success(self, lock):
        """Test successful lock release."""
        lock._acquired = True
        lock._holder = "bobbin-test"

        with patch.object(lock, "status") as mock_status, \
             patch.object(lock, "_run_git") as mock_git:
            # Mock status to show lock held by us
            mock_status.return_value = LockStatus(
                held=True,
                metadata=LockMetadata(
                    holder="bobbin-test",
                    acquired_at=datetime.now(timezone.utc),
                    host="test",
                    pid=1234,
                ),
            )
            mock_git.return_value = MagicMock(returncode=0)

            result = lock.release("bobbin-test")

            assert result is True
            assert not lock.is_acquired
            mock_git.assert_called_with("push", "origin", "--delete", LOCK_REF, check=False)

    def test_release_wrong_holder(self, lock):
        """Test release fails when different holder."""
        lock._acquired = True
        lock._holder = "bobbin-test"

        with patch.object(lock, "status") as mock_status:
            mock_status.return_value = LockStatus(
                held=True,
                metadata=LockMetadata(
                    holder="bobbin-other",
                    acquired_at=datetime.now(timezone.utc),
                    host="test",
                    pid=1234,
                ),
            )

            result = lock.release("bobbin-test")

            assert result is False

    def test_force_release(self, lock):
        """Test force release without ownership check."""
        with patch.object(lock, "_run_git") as mock_git:
            mock_git.return_value = MagicMock(returncode=0)

            result = lock.force_release()

            assert result is True
            mock_git.assert_called_with("push", "origin", "--delete", LOCK_REF, check=False)

    def test_context_manager_releases_on_exit(self, lock):
        """Test that context manager releases lock on exit."""
        lock._acquired = True
        lock._holder = "bobbin-test"

        with patch.object(lock, "release") as mock_release:
            mock_release.return_value = True

            with lock:
                pass

            mock_release.assert_called_once_with("bobbin-test")
