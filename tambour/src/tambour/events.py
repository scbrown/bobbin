"""Event types and dispatcher for tambour.

Defines the lifecycle events that tambour emits and the mechanism
for dispatching them to configured plugins.
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config, PluginConfig


class EventType(Enum):
    """Lifecycle events emitted by tambour."""

    AGENT_SPAWNED = "agent.spawned"
    AGENT_FINISHED = "agent.finished"
    BRANCH_MERGED = "branch.merged"
    TASK_CLAIMED = "task.claimed"
    TASK_COMPLETED = "task.completed"
    HEALTH_ZOMBIE = "health.zombie"


@dataclass
class Event:
    """An event to be dispatched to plugins.

    Attributes:
        event_type: The type of event.
        issue_id: The issue ID associated with the event.
        issue_title: The issue title.
        issue_type: The issue type (task, bug, etc.).
        branch: The git branch name.
        worktree: Path to the worktree.
        main_repo: Path to the main repository.
        beads_db: Path to the beads database.
        timestamp: When the event occurred.
        extra: Additional event-specific data.
    """

    event_type: EventType
    issue_id: str | None = None
    issue_title: str | None = None
    issue_type: str | None = None
    branch: str | None = None
    worktree: Path | None = None
    main_repo: Path | None = None
    beads_db: Path | None = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    extra: dict[str, str] = field(default_factory=dict)

    def to_env(self) -> dict[str, str]:
        """Convert event to environment variables for plugin execution."""
        env: dict[str, str] = {
            "TAMBOUR_EVENT": self.event_type.value,
            "TAMBOUR_TIMESTAMP": self.timestamp.isoformat(),
        }

        if self.issue_id:
            env["TAMBOUR_ISSUE_ID"] = self.issue_id
        if self.issue_title:
            env["TAMBOUR_ISSUE_TITLE"] = self.issue_title
        if self.issue_type:
            env["TAMBOUR_ISSUE_TYPE"] = self.issue_type
        if self.branch:
            env["TAMBOUR_BRANCH"] = self.branch
        if self.worktree:
            env["TAMBOUR_WORKTREE"] = str(self.worktree.absolute())
        if self.main_repo:
            env["TAMBOUR_MAIN_REPO"] = str(self.main_repo.absolute())
        if self.beads_db:
            env["TAMBOUR_BEADS_DB"] = str(self.beads_db.absolute())

        # Add extra event-specific variables
        for key, value in self.extra.items():
            env_key = f"TAMBOUR_{key.upper()}"
            env[env_key] = value

        return env


@dataclass
class PluginResult:
    """Result of executing a plugin."""

    plugin_name: str
    success: bool
    exit_code: int | None = None
    error: str | None = None
    output: str | None = None
    duration_ms: int | None = None


class EventDispatcher:
    """Dispatches events to configured plugins."""

    def __init__(self, config: Config):
        """Initialize the dispatcher with configuration.

        Args:
            config: The tambour configuration.
        """
        self.config = config

    def dispatch(self, event: Event) -> list[PluginResult]:
        """Dispatch an event to all configured plugins.

        Args:
            event: The event to dispatch.

        Returns:
            List of results from each plugin execution.
        """
        plugins = self.config.get_plugins_for_event(event.event_type.value)
        results: list[PluginResult] = []

        for plugin in plugins:
            result = self._execute_plugin(plugin, event)
            results.append(result)

            # Stop on blocking plugin failure
            if plugin.blocking and not result.success:
                break

        return results

    def _execute_plugin(self, plugin: PluginConfig, event: Event) -> PluginResult:
        """Execute a single plugin.

        Args:
            plugin: The plugin configuration.
            event: The event being dispatched.

        Returns:
            Result of the plugin execution.
        """
        # Build environment with event data
        env = os.environ.copy()
        env.update(event.to_env())

        start_time = datetime.now()

        try:
            result = subprocess.run(
                plugin.run,
                shell=True,
                env=env,
                capture_output=True,
                text=True,
                timeout=plugin.timeout,
                cwd=event.worktree or event.main_repo,
            )

            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)

            return PluginResult(
                plugin_name=plugin.name,
                success=result.returncode == 0,
                exit_code=result.returncode,
                output=result.stdout,
                error=result.stderr if result.returncode != 0 else None,
                duration_ms=duration_ms,
            )

        except subprocess.TimeoutExpired:
            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)
            return PluginResult(
                plugin_name=plugin.name,
                success=False,
                error=f"Plugin timed out after {plugin.timeout}s",
                duration_ms=duration_ms,
            )

        except Exception as e:
            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)
            return PluginResult(
                plugin_name=plugin.name,
                success=False,
                error=str(e),
                duration_ms=duration_ms,
            )
