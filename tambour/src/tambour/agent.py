"""Agent spawner for tambour.

Spawns AI agents (claude/gemini) on beads issues with worktree isolation.
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from tambour.config import Config
from tambour.context import ContextCollector, ContextRequest
from tambour.events import Event, EventDispatcher, EventType


@dataclass
class AgentConfig:
    """Configuration for agent spawning."""

    cli: str
    issue_id: str
    worktree_path: Path
    main_repo: Path
    title: str
    completion_context: str | None = None


class BeadsClient:
    """Client for interacting with beads (bd) CLI."""

    @staticmethod
    def get_ready_issues(label: str | None = None) -> list[dict[str, Any]]:
        """Get ready issues from beads.

        Args:
            label: Optional label to filter by.

        Returns:
            List of ready issue dictionaries.
        """
        result = subprocess.run(
            ["bd", "ready", "--json"],
            capture_output=True,
            text=True,
            check=True,
        )
        issues = json.loads(result.stdout)

        # Filter to tasks only (not epics)
        issues = [i for i in issues if i.get("issue_type") == "task"]

        # Filter by label if specified
        if label:
            issues = [
                i for i in issues if label in (i.get("labels") or [])
            ]

        return issues

    @staticmethod
    def get_issue(issue_id: str) -> dict[str, Any]:
        """Get issue details from beads.

        Args:
            issue_id: The issue ID.

        Returns:
            Issue dictionary.

        Raises:
            subprocess.CalledProcessError: If issue not found.
        """
        result = subprocess.run(
            ["bd", "show", issue_id, "--json"],
            capture_output=True,
            text=True,
            check=True,
        )
        issues = json.loads(result.stdout)
        if not issues:
            raise ValueError(f"Issue not found: {issue_id}")
        return issues[0]

    @staticmethod
    def show_issue(issue_id: str) -> str:
        """Get human-readable issue output.

        Args:
            issue_id: The issue ID.

        Returns:
            Formatted issue output string.
        """
        result = subprocess.run(
            ["bd", "show", issue_id],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout

    @staticmethod
    def claim_issue(issue_id: str) -> bool:
        """Claim an issue (set status to in_progress and assign).

        Args:
            issue_id: The issue ID.

        Returns:
            True if claim succeeded, False otherwise.
        """
        result = subprocess.run(
            ["bd", "update", issue_id, "--claim"],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0

    @staticmethod
    def unclaim_issue(issue_id: str) -> bool:
        """Unclaim an issue (set status to open and clear assignee).

        Args:
            issue_id: The issue ID.

        Returns:
            True if unclaim succeeded, False otherwise.
        """
        result = subprocess.run(
            ["bd", "update", issue_id, "--status", "open", "--assignee", ""],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0

    @staticmethod
    def create_worktree(path: Path, branch: str) -> bool:
        """Create a worktree for an issue.

        Args:
            path: Path to create worktree at.
            branch: Branch name (typically issue ID).

        Returns:
            True if creation succeeded, False otherwise.
        """
        result = subprocess.run(
            ["bd", "worktree", "create", str(path), "--branch", branch],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0


class AgentSpawner:
    """Spawns AI agents on beads issues."""

    def __init__(self, config: Config, main_repo: Path | None = None):
        """Initialize agent spawner.

        Args:
            config: Tambour configuration.
            main_repo: Path to main repository. If None, uses current directory.
        """
        self.config = config
        self.main_repo = main_repo or Path.cwd()
        self.beads = BeadsClient()
        self.event_dispatcher = EventDispatcher(config)
        self.context_collector = ContextCollector(config)

        # Track state for cleanup
        self._claimed_issue: str | None = None
        self._heartbeat_proc: subprocess.Popen | None = None

    def _get_worktree_base(self) -> Path:
        """Get the base path for worktrees.

        Returns:
            Resolved worktree base path.
        """
        base_template = self.config.worktree.base_path
        repo_name = self.main_repo.name
        base_path = base_template.replace("{repo}", repo_name)

        # Resolve relative to main repo
        if not Path(base_path).is_absolute():
            base_path = str(self.main_repo / base_path)

        return Path(base_path).resolve()

    def _get_worktree_path(self, issue_id: str) -> Path:
        """Get the worktree path for an issue.

        Args:
            issue_id: The issue ID.

        Returns:
            Path to the worktree.
        """
        return self._get_worktree_base() / issue_id

    def _emit_event(
        self,
        event_type: EventType,
        issue_id: str,
        worktree: Path | None = None,
        extra: dict[str, str] | None = None,
    ) -> None:
        """Emit an event.

        Args:
            event_type: Type of event.
            issue_id: The issue ID.
            worktree: Path to worktree.
            extra: Additional event data.
        """
        event = Event(
            event_type=event_type,
            issue_id=issue_id,
            worktree=worktree,
            main_repo=self.main_repo,
            extra=extra or {},
        )
        self.event_dispatcher.dispatch(event)

    def _build_prompt(
        self,
        issue_id: str,
        worktree_path: Path,
        completion_context: str | None = None,
    ) -> str:
        """Build the agent prompt.

        Args:
            issue_id: The issue ID.
            worktree_path: Path to the worktree.
            completion_context: Optional context from previous session.

        Returns:
            The complete prompt string.
        """
        bd_show_output = self.beads.show_issue(issue_id)

        context_prefix = ""
        if completion_context:
            context_prefix = f"{completion_context}\n\n---\n\n"

        base_prompt = f"""{context_prefix}You have been assigned to work on a beads issue. Here's what was executed to show you the task:

$ bd show {issue_id}
{bd_show_output}
You are now in a git worktree at: {worktree_path}
Branch: {issue_id}

Begin working on this task now:
1. Read CLAUDE.md and any relevant docs to understand the project
2. Explore the codebase to understand what exists and what you need to build
3. Implement the task, committing your changes as you go
4. When complete, inform the user the task is ready for review

Start immediately - do not ask for confirmation."""

        # Collect context from providers
        request = ContextRequest(
            prompt=base_prompt,
            issue_id=issue_id,
            worktree=worktree_path,
            main_repo=self.main_repo,
        )
        injected_context, _ = self.context_collector.collect(request)

        if injected_context:
            return f"{base_prompt}\n\n---\n\n{injected_context}"
        return base_prompt

    def _start_heartbeat(self, worktree_path: Path) -> subprocess.Popen | None:
        """Start the heartbeat writer process.

        Args:
            worktree_path: Path to the worktree.

        Returns:
            The heartbeat process, or None if failed.
        """
        try:
            proc = subprocess.Popen(
                [sys.executable, "-m", "tambour", "heartbeat", str(worktree_path)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            return proc
        except Exception:
            return None

    def _stop_heartbeat(self) -> None:
        """Stop the heartbeat writer process."""
        if self._heartbeat_proc:
            try:
                self._heartbeat_proc.terminate()
                self._heartbeat_proc.wait(timeout=5)
            except Exception:
                try:
                    self._heartbeat_proc.kill()
                except Exception:
                    pass
            self._heartbeat_proc = None

    def _cleanup_on_failure(self) -> None:
        """Cleanup on failure - unclaim issue and stop heartbeat."""
        self._stop_heartbeat()
        if self._claimed_issue:
            print(f"\nScript failed - unclaiming {self._claimed_issue}...")
            self.beads.unclaim_issue(self._claimed_issue)
            self._claimed_issue = None

    def _run_health_check(self) -> None:
        """Run startup health check for zombied tasks."""
        health_script = self.main_repo / "scripts" / "health-check.sh"
        if health_script.exists():
            result = subprocess.run(
                [str(health_script)],
                capture_output=True,
                text=True,
            )
            if "ZOMBIE" in result.stdout:
                print(
                    "Warning: Found zombied tasks. "
                    "Run './scripts/health-check.sh --fix' to clean up.\n"
                )

    def select_issue(
        self, issue_id: str | None = None, label: str | None = None
    ) -> tuple[str, str]:
        """Select an issue to work on.

        Args:
            issue_id: Specific issue ID, or None to pick next ready task.
            label: Filter ready tasks by label.

        Returns:
            Tuple of (issue_id, issue_title).

        Raises:
            ValueError: If no issue found.
        """
        if issue_id:
            issue = self.beads.get_issue(issue_id)
            return issue["id"], issue["title"]

        # Get next ready task
        ready_issues = self.beads.get_ready_issues(label=label)
        if not ready_issues:
            label_msg = f" with label '{label}'" if label else ""
            raise ValueError(f"No ready tasks available{label_msg}")

        issue = ready_issues[0]
        return issue["id"], issue["title"]

    def spawn(
        self,
        cli: str | None = None,
        issue_id: str | None = None,
        label: str | None = None,
        completion_context: str | None = None,
    ) -> int:
        """Spawn an agent on an issue.

        Args:
            cli: CLI to use (claude/gemini). If None, uses config default.
            issue_id: Specific issue ID, or None to pick next ready task.
            label: Filter ready tasks by label.
            completion_context: Optional context from previous session.

        Returns:
            Exit code from the agent.
        """
        # Run health check
        self._run_health_check()

        # Resolve CLI
        agent_cli = cli or self.config.agent.default_cli
        if not agent_cli:
            print("Note: No agent CLI specified or configured. Defaulting to 'claude'.")
            agent_cli = "claude"

        # Select issue
        try:
            selected_id, title = self.select_issue(issue_id, label)
        except ValueError as e:
            print(str(e))
            # Show ready tasks
            subprocess.run(["bd", "ready"])
            return 1

        worktree_path = self._get_worktree_path(selected_id)

        print(f"=== Starting agent for: {selected_id} ===")
        print(f"Title: {title}")
        print()

        # Create worktree if needed
        if worktree_path.exists():
            print("Worktree already exists, reusing...")
        else:
            print("Creating worktree with beads redirect...")
            if not self.beads.create_worktree(worktree_path, selected_id):
                print(f"Error: Failed to create worktree at {worktree_path}")
                return 1

            self._emit_event(EventType.AGENT_SPAWNED, selected_id, worktree_path)

        # Claim the issue
        print(f"Claiming {selected_id}...")
        if self.beads.claim_issue(selected_id):
            self._claimed_issue = selected_id
            self._emit_event(EventType.TASK_CLAIMED, selected_id, worktree_path)
        else:
            print("Warning: Could not claim issue (may already be claimed)")

        print()
        print(f"=== Launching {agent_cli} in worktree ===")
        print(f"Path: {worktree_path}")
        print()

        # Build prompt
        prompt = self._build_prompt(selected_id, worktree_path, completion_context)

        # Start heartbeat
        self._heartbeat_proc = self._start_heartbeat(worktree_path)

        # Set up signal handlers for cleanup
        original_sigint = signal.signal(signal.SIGINT, signal.SIG_IGN)
        original_sigterm = signal.signal(signal.SIGTERM, signal.SIG_IGN)

        try:
            # Write prompt to temp file for gemini
            with tempfile.NamedTemporaryFile(
                mode="w", suffix=".txt", delete=False
            ) as f:
                f.write(prompt)
                prompt_file = f.name

            try:
                # Run agent in worktree
                if agent_cli.startswith("gemini"):
                    cmd = [agent_cli, "-i", prompt]
                else:
                    cmd = [agent_cli, prompt]

                result = subprocess.run(cmd, cwd=worktree_path)
                agent_exit = result.returncode
            finally:
                Path(prompt_file).unlink(missing_ok=True)

        except Exception as e:
            print(f"Error running agent: {e}")
            self._cleanup_on_failure()
            return 1
        finally:
            signal.signal(signal.SIGINT, original_sigint)
            signal.signal(signal.SIGTERM, original_sigterm)
            self._stop_heartbeat()

        # Emit agent.finished event
        self._emit_event(
            EventType.AGENT_FINISHED,
            selected_id,
            worktree_path,
            {"exit_code": str(agent_exit)},
        )

        # Handle exit
        if agent_exit == 0:
            self._claimed_issue = None  # Don't unclaim on success
        else:
            print()
            print(f"{agent_cli} exited with code {agent_exit}")
            self._cleanup_on_failure()

        # Check if worktree still exists
        if worktree_path.exists():
            print()
            print("=" * 74)
            print(f"Warning: Worktree still exists at: {worktree_path}")
            print()
            print("The agent session ended but the task wasn't merged.")
            print("To finish and merge the task, run:")
            print()
            print(f"    just tambour finish {selected_id}")
            print()
            print("Or to abort and discard changes:")
            print()
            print(f"    just tambour abort {selected_id}")
            print("=" * 74)

        return agent_exit
