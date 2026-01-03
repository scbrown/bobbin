"""Health check logic for tambour.

Detects zombie tasks (in_progress but no active agent) and
optionally triggers recovery actions.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config


@dataclass
class TaskHealth:
    """Health status of a task."""

    issue_id: str
    status: str
    assignee: str | None
    worktree_path: Path | None
    worktree_exists: bool
    is_zombie: bool
    last_activity: datetime | None = None


class HealthChecker:
    """Checks health of active tasks and worktrees.

    A task is considered a "zombie" if:
    - It has status "in_progress"
    - Its worktree no longer exists, OR
    - No heartbeat detected within the zombie threshold
    """

    def __init__(self, config: Config):
        """Initialize the health checker.

        Args:
            config: The tambour configuration.
        """
        self.config = config
        self.zombie_threshold = config.daemon.zombie_threshold
        self.auto_recover = config.daemon.auto_recover

    def check_all(self) -> list[TaskHealth]:
        """Check health of all in-progress tasks.

        Returns:
            List of health status for each in-progress task.
        """
        tasks = self._get_in_progress_tasks()
        results: list[TaskHealth] = []

        for task in tasks:
            health = self._check_task(task)
            results.append(health)

            if health.is_zombie:
                self._handle_zombie(health)

        return results

    def check_task(self, issue_id: str) -> TaskHealth | None:
        """Check health of a specific task.

        Args:
            issue_id: The issue ID to check.

        Returns:
            Health status, or None if task not found.
        """
        task = self._get_task(issue_id)
        if task is None:
            return None

        return self._check_task(task)

    def _get_in_progress_tasks(self) -> list[dict[str, str]]:
        """Get all tasks with in_progress status.

        Returns:
            List of task dictionaries from beads.
        """
        try:
            result = subprocess.run(
                ["bd", "list", "--status", "in_progress", "--format", "json"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if result.returncode != 0:
                return []

            # Parse JSON output
            import json

            return json.loads(result.stdout)
        except (subprocess.TimeoutExpired, FileNotFoundError, json.JSONDecodeError):
            return []

    def _get_task(self, issue_id: str) -> dict[str, str] | None:
        """Get a specific task by ID.

        Args:
            issue_id: The issue ID.

        Returns:
            Task dictionary, or None if not found.
        """
        try:
            result = subprocess.run(
                ["bd", "show", issue_id, "--format", "json"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if result.returncode != 0:
                return None

            import json

            return json.loads(result.stdout)
        except (subprocess.TimeoutExpired, FileNotFoundError, json.JSONDecodeError):
            return None

    def _check_task(self, task: dict[str, str]) -> TaskHealth:
        """Check the health of a single task.

        Args:
            task: Task dictionary from beads.

        Returns:
            Health status for the task.
        """
        issue_id = task.get("id", "")
        status = task.get("status", "")
        assignee = task.get("assignee")

        # Check if worktree exists
        worktree_path = self._find_worktree(issue_id)
        worktree_exists = worktree_path is not None and worktree_path.exists()

        # Check if agent process is running (if worktree exists)
        process_running = False
        if worktree_exists and worktree_path:
            process_running = self._is_process_running_in_worktree(worktree_path)

        # A task is a zombie if it's in_progress but:
        # 1. Has no worktree, OR
        # 2. Has a worktree but no agent process running in it
        is_zombie = status == "in_progress" and (not worktree_exists or not process_running)

        return TaskHealth(
            issue_id=issue_id,
            status=status,
            assignee=assignee,
            worktree_path=worktree_path,
            worktree_exists=worktree_exists,
            is_zombie=is_zombie,
        )

    def _is_process_running_in_worktree(self, worktree_path: Path) -> bool:
        """Check if any process has the worktree as its CWD.

        Uses lsof to find processes with CWD in the given path.
        """
        try:
            # lsof +d <path> lists open files in path
            # grep " cwd " filters for Current Working Directory
            cmd = f"lsof +d '{worktree_path}' | grep ' cwd '"
            result = subprocess.run(
                cmd,
                shell=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=2,
            )
            return result.returncode == 0
        except subprocess.TimeoutExpired:
            return False

    def _find_worktree(self, issue_id: str) -> Path | None:
        """Find the worktree path for a task.

        Args:
            issue_id: The issue ID (also branch name).

        Returns:
            Path to worktree, or None if not found.
        """
        # Get worktree base path from config
        base_path_template = self.config.worktree.base_path

        # Get repo name from current directory
        cwd = Path.cwd()
        repo_name = cwd.name

        # Expand template
        base_path = base_path_template.replace("{repo}", repo_name)

        # Construct worktree path
        worktree_path = cwd.parent / base_path.lstrip("../") / issue_id

        return worktree_path if worktree_path.exists() else None

    def _handle_zombie(self, health: TaskHealth) -> None:
        """Handle a zombie task.

        Args:
            health: The health status of the zombie task.
        """
        from tambour.events import Event, EventType, EventDispatcher

        # Emit health.zombie event
        event = Event(
            event_type=EventType.HEALTH_ZOMBIE,
            issue_id=health.issue_id,
            worktree=health.worktree_path,
            extra={
                "worktree_exists": str(health.worktree_exists).lower(),
            },
        )

        dispatcher = EventDispatcher(self.config)
        dispatcher.dispatch(event)

        # Auto-recover if configured
        if self.auto_recover:
            self._recover_zombie(health)

    def _recover_zombie(self, health: TaskHealth) -> bool:
        """Attempt to recover a zombie task.

        Args:
            health: The health status of the zombie task.

        Returns:
            True if recovery succeeded.
        """
        try:
            # Unclaim the task by removing assignee
            result = subprocess.run(
                ["bd", "update", health.issue_id, "--status", "open"],
                capture_output=True,
                text=True,
                timeout=10,
            )
            return result.returncode == 0
        except (subprocess.TimeoutExpired, FileNotFoundError):
            return False
