"""Event types and dispatcher for tambour.

Defines the lifecycle events that tambour emits and the mechanism
for dispatching them to configured plugins.
"""

from __future__ import annotations

import os
import subprocess
import sys
import threading
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config, PluginConfig


class EventType(Enum):
    """Lifecycle events emitted by tambour."""

    # Lifecycle events
    AGENT_SPAWNED = "agent.spawned"
    AGENT_FINISHED = "agent.finished"
    BRANCH_MERGED = "branch.merged"
    TASK_CLAIMED = "task.claimed"
    TASK_COMPLETED = "task.completed"
    HEALTH_ZOMBIE = "health.zombie"

    # Tool use events
    TOOL_USED = "tool.used"
    TOOL_FAILED = "tool.failed"

    # Session events
    SESSION_STARTED = "session.started"
    SESSION_FILE_READ = "session.file_read"
    SESSION_FILE_WRITTEN = "session.file_written"


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
class ToolEvent:
    """Event data for tool.used and tool.failed events.

    Captures information about tool invocations from Claude Code sessions,
    enabling metrics collection and analysis.

    Attributes:
        tool_name: Name of the tool (Read, Write, Edit, Bash, etc.).
        tool_input: Parameters passed to the tool.
        tool_response: Response/result from the tool.
        session_id: Claude Code session identifier.
        timestamp: When the tool was invoked.
        issue_id: Associated beads issue ID (if in a worktree).
        worktree: Path to the worktree (if applicable).
        duration_ms: Tool execution time in milliseconds.
        error: Error message if the tool failed.
    """

    tool_name: str
    tool_input: dict[str, str]
    tool_response: dict[str, str]
    session_id: str
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    issue_id: str | None = None
    worktree: Path | None = None
    duration_ms: int | None = None
    error: str | None = None

    def to_event(self, failed: bool = False) -> Event:
        """Convert to a standard Event for dispatching.

        Args:
            failed: If True, use TOOL_FAILED event type; otherwise TOOL_USED.

        Returns:
            An Event instance suitable for the event dispatcher.
        """
        event_type = EventType.TOOL_FAILED if failed else EventType.TOOL_USED
        extra = {
            "tool_name": self.tool_name,
            "session_id": self.session_id,
        }
        if self.duration_ms is not None:
            extra["duration_ms"] = str(self.duration_ms)
        if self.error:
            extra["error"] = self.error

        return Event(
            event_type=event_type,
            issue_id=self.issue_id,
            worktree=self.worktree,
            timestamp=self.timestamp,
            extra=extra,
        )


@dataclass
class SessionEvent:
    """Event data for session.* events.

    Captures session lifecycle and file operation events.

    Attributes:
        session_id: Claude Code session identifier.
        timestamp: When the event occurred.
        issue_id: Associated beads issue ID (if in a worktree).
        worktree: Path to the worktree (if applicable).
        file_path: Path to the file (for file_read/file_written events).
        lines: Number of lines read (for file_read events).
        success: Whether the operation succeeded.
    """

    session_id: str
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    issue_id: str | None = None
    worktree: Path | None = None
    file_path: Path | None = None
    lines: int | None = None
    success: bool = True

    def to_event(self, event_type: EventType) -> Event:
        """Convert to a standard Event for dispatching.

        Args:
            event_type: The session event type (SESSION_STARTED, SESSION_FILE_READ, etc.).

        Returns:
            An Event instance suitable for the event dispatcher.
        """
        extra = {
            "session_id": self.session_id,
        }
        if self.file_path:
            extra["file_path"] = str(self.file_path)
        if self.lines is not None:
            extra["lines"] = str(self.lines)
        extra["success"] = str(self.success).lower()

        return Event(
            event_type=event_type,
            issue_id=self.issue_id,
            worktree=self.worktree,
            timestamp=self.timestamp,
            extra=extra,
        )


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

    def __init__(self, config: Config, log_file: Path | None = None):
        """Initialize the dispatcher with configuration.

        Args:
            config: The tambour configuration.
            log_file: Optional path to log file for async results.
        """
        self.config = config
        self.log_file = log_file

    def dispatch(self, event: Event) -> list[PluginResult]:
        """Dispatch an event to all configured plugins.

        Args:
            event: The event to dispatch.

        Returns:
            List of results from each plugin execution.
            For non-blocking plugins, returns a placeholder result.
        """
        plugins = self.config.get_plugins_for_event(event.event_type.value)
        results: list[PluginResult] = []

        for plugin in plugins:
            if plugin.blocking:
                result = self._execute_plugin(plugin, event)
                self._log_result(result)
                results.append(result)

                # Stop on blocking plugin failure
                if not result.success:
                    break
            else:
                # Fire and forget (async)
                self._dispatch_async(plugin, event)
                
                # Return placeholder
                results.append(PluginResult(
                    plugin_name=plugin.name,
                    success=True,
                    output="(Async execution started)",
                    duration_ms=0,
                ))

        return results

    def _dispatch_async(self, plugin: PluginConfig, event: Event) -> None:
        """Run a plugin in a background thread."""
        def task() -> None:
            result = self._execute_plugin(plugin, event)
            self._log_result(result)

        # Use daemon thread so it doesn't block program exit if needed,
        # but note that this means operations might be cut short on CLI exit.
        # Given "fire-and-forget", this is often acceptable behavior for CLIs,
        # or the user should run via the daemon.
        thread = threading.Thread(target=task, daemon=False)
        thread.start()

    def _log_result(self, result: PluginResult) -> None:
        """Log the result of a plugin execution."""
        if not self.log_file:
            return

        timestamp = datetime.now(timezone.utc).isoformat()
        status = "SUCCESS" if result.success else "FAILED"
        
        try:
            with open(self.log_file, "a") as f:
                f.write(f"[{timestamp}] [{status}] Plugin '{result.plugin_name}': ")
                if result.exit_code is not None:
                    f.write(f"exit_code={result.exit_code} ")
                f.write(f"duration={result.duration_ms}ms\n")
                if result.error:
                    f.write(f"  Error: {result.error}\n")
        except Exception as e:
            print(f"Failed to write to log file: {e}", file=sys.stderr)

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
