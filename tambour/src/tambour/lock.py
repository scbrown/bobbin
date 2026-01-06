"""Distributed merge lock using git refs.

Provides a mechanism to serialize merges across parallel agents
by using a git ref on the remote as a distributed lock.
"""

from __future__ import annotations

import json
import os
import socket
import subprocess
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


LOCK_REF = "refs/tambour/merge-lock"
DEFAULT_TIMEOUT = 300  # 5 minutes
POLL_INTERVAL = 5  # seconds


@dataclass
class LockMetadata:
    """Metadata stored in the lock commit."""

    holder: str
    acquired_at: datetime
    host: str
    pid: int

    def to_dict(self) -> dict:
        """Convert to dictionary for JSON serialization."""
        return {
            "holder": self.holder,
            "acquired_at": self.acquired_at.isoformat(),
            "host": self.host,
            "pid": self.pid,
        }

    @classmethod
    def from_dict(cls, data: dict) -> LockMetadata:
        """Create from dictionary."""
        return cls(
            holder=data["holder"],
            acquired_at=datetime.fromisoformat(data["acquired_at"]),
            host=data["host"],
            pid=data["pid"],
        )


@dataclass
class LockStatus:
    """Status of the merge lock."""

    held: bool
    metadata: LockMetadata | None = None

    @property
    def holder(self) -> str | None:
        """Get the holder name if lock is held."""
        return self.metadata.holder if self.metadata else None


class MergeLock:
    """Distributed merge lock using git refs.

    The lock is stored as a git ref (refs/tambour/merge-lock) on the remote.
    Acquiring the lock creates a commit with metadata and pushes it to the ref.
    If the ref already exists, the push fails atomically.
    """

    def __init__(self, repo_path: Path, timeout: int | None = None):
        """Initialize the merge lock.

        Args:
            repo_path: Path to the git repository.
            timeout: Maximum seconds to wait for lock. Defaults to TAMBOUR_LOCK_TIMEOUT
                     env var or 300 seconds.
        """
        self.repo_path = repo_path
        self.timeout = timeout or int(os.environ.get("TAMBOUR_LOCK_TIMEOUT", DEFAULT_TIMEOUT))
        self._acquired = False
        self._holder: str | None = None

    def _run_git(self, *args: str, check: bool = True) -> subprocess.CompletedProcess:
        """Run a git command in the repo."""
        return subprocess.run(
            ["git", "-C", str(self.repo_path), *args],
            capture_output=True,
            text=True,
            check=check,
        )

    def status(self) -> LockStatus:
        """Check the current lock status.

        Returns:
            LockStatus indicating if lock is held and by whom.
        """
        # Try to fetch the lock ref
        result = self._run_git("fetch", "origin", LOCK_REF, check=False)
        if result.returncode != 0:
            return LockStatus(held=False)

        # Read the lock metadata
        try:
            cat_result = self._run_git("cat-file", "-p", "FETCH_HEAD", check=False)
            if cat_result.returncode != 0:
                return LockStatus(held=True)

            # Parse the commit tree to find lock.json
            tree_result = self._run_git("ls-tree", "FETCH_HEAD", check=False)
            if tree_result.returncode != 0:
                return LockStatus(held=True)

            # Extract blob hash for lock.json
            for line in tree_result.stdout.strip().split("\n"):
                if "lock.json" in line:
                    parts = line.split()
                    if len(parts) >= 3:
                        blob_hash = parts[2]
                        blob_result = self._run_git("cat-file", "-p", blob_hash, check=False)
                        if blob_result.returncode == 0:
                            data = json.loads(blob_result.stdout)
                            metadata = LockMetadata.from_dict(data)
                            return LockStatus(held=True, metadata=metadata)

            return LockStatus(held=True)
        except (json.JSONDecodeError, KeyError):
            return LockStatus(held=True)

    def acquire(self, holder: str) -> bool:
        """Acquire the merge lock.

        Blocks until the lock is acquired or timeout is reached.

        Args:
            holder: Identifier for the lock holder (usually issue ID).

        Returns:
            True if lock was acquired, False on timeout.
        """
        deadline = time.time() + self.timeout

        # Create lock metadata
        metadata = LockMetadata(
            holder=holder,
            acquired_at=datetime.now(timezone.utc),
            host=socket.gethostname(),
            pid=os.getpid(),
        )
        lock_data = json.dumps(metadata.to_dict(), indent=2)

        while time.time() < deadline:
            # Create git objects for the lock
            try:
                # Create blob with lock data
                blob_result = subprocess.run(
                    ["git", "-C", str(self.repo_path), "hash-object", "-w", "--stdin"],
                    input=lock_data,
                    capture_output=True,
                    text=True,
                    check=True,
                )
                blob_sha = blob_result.stdout.strip()

                # Create tree with the blob
                tree_input = f"100644 blob {blob_sha}\tlock.json\n"
                tree_result = subprocess.run(
                    ["git", "-C", str(self.repo_path), "mktree"],
                    input=tree_input,
                    capture_output=True,
                    text=True,
                    check=True,
                )
                tree_sha = tree_result.stdout.strip()

                # Create commit with the tree
                commit_result = subprocess.run(
                    ["git", "-C", str(self.repo_path), "commit-tree", tree_sha, "-m", f"merge lock: {holder}"],
                    capture_output=True,
                    text=True,
                    check=True,
                )
                commit_sha = commit_result.stdout.strip()

                # Try to push the commit as the lock ref
                push_result = self._run_git(
                    "push", "origin", f"{commit_sha}:{LOCK_REF}",
                    check=False,
                )

                if push_result.returncode == 0:
                    self._acquired = True
                    self._holder = holder
                    return True

                # Lock is held by someone else, check who
                status = self.status()
                current_holder = status.holder or "unknown"
                print(f"Waiting for merge lock (held by {current_holder})...")
                time.sleep(POLL_INTERVAL)

            except subprocess.CalledProcessError as e:
                print(f"Error acquiring lock: {e}")
                time.sleep(POLL_INTERVAL)

        return False

    def release(self, holder: str | None = None) -> bool:
        """Release the merge lock.

        Args:
            holder: Expected holder name. If provided, verifies ownership before release.

        Returns:
            True if lock was released, False otherwise.
        """
        if holder:
            # Verify we hold the lock
            status = self.status()
            if status.held and status.holder != holder:
                print(f"ERROR: Lock held by '{status.holder}', not '{holder}'")
                return False

        # Delete the ref
        result = self._run_git("push", "origin", "--delete", LOCK_REF, check=False)

        if result.returncode == 0:
            self._acquired = False
            self._holder = None
            return True

        # Lock might already be released
        if "unable to delete" in result.stderr.lower() or "remote ref does not exist" in result.stderr.lower():
            self._acquired = False
            self._holder = None
            return True

        return False

    def force_release(self) -> bool:
        """Force-release the lock without ownership verification.

        Use for recovery when a lock is stuck.

        Returns:
            True if lock was released or didn't exist.
        """
        result = self._run_git("push", "origin", "--delete", LOCK_REF, check=False)
        self._acquired = False
        self._holder = None
        return result.returncode == 0 or "remote ref does not exist" in result.stderr.lower()

    @property
    def is_acquired(self) -> bool:
        """Check if this instance currently holds the lock."""
        return self._acquired

    def __enter__(self) -> MergeLock:
        """Context manager entry - does NOT auto-acquire (explicit acquire required)."""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        """Context manager exit - releases lock if held."""
        if self._acquired and self._holder:
            self.release(self._holder)
