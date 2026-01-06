"""Finish command for tambour.

Implements the workflow to merge agent work and complete an issue:
1. Acquire distributed merge lock
2. Switch to main repo, pull latest
3. Merge feature branch (--no-ff)
4. Push to remote
5. Close the issue via bd close
6. Remove worktree and delete branch
7. Release merge lock
8. Optionally offer to start next task
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from tambour.lock import MergeLock

if TYPE_CHECKING:
    from tambour.config import Config


@dataclass
class FinishResult:
    """Result of the finish operation."""

    success: bool
    issue_id: str
    issue_title: str = ""
    merged: bool = False
    worktree_removed: bool = False
    branch_deleted: bool = False
    issue_closed: bool = False
    closed_epics: list[tuple[str, str]] = None  # (id, title) pairs
    error: str | None = None

    def __post_init__(self):
        if self.closed_epics is None:
            self.closed_epics = []


class FinishCommand:
    """Handles the finish workflow for completing agent work."""

    def __init__(
        self,
        issue_id: str,
        main_repo: Path,
        worktree_base: Path | None = None,
        merge: bool = True,
        no_continue: bool = False,
        config: Config | None = None,
    ):
        """Initialize the finish command.

        Args:
            issue_id: The issue ID to finish.
            main_repo: Path to the main git repository.
            worktree_base: Base path for worktrees. Defaults to ../bobbin-worktrees.
            merge: Whether to merge the branch into main.
            no_continue: Skip the "continue to next task" flow.
            config: Optional tambour configuration.
        """
        self.issue_id = issue_id
        self.main_repo = main_repo.resolve()
        self.worktree_base = worktree_base or (self.main_repo.parent / "bobbin-worktrees")
        self.worktree_path = self.worktree_base / issue_id
        self.branch_name = issue_id
        self.merge = merge
        self.no_continue = no_continue
        self.config = config
        self._lock: MergeLock | None = None

    def _run_git(self, *args: str, cwd: Path | None = None, check: bool = True) -> subprocess.CompletedProcess:
        """Run a git command."""
        return subprocess.run(
            ["git", *args],
            cwd=cwd or self.main_repo,
            capture_output=True,
            text=True,
            check=check,
        )

    def _run_bd(self, *args: str, check: bool = True) -> subprocess.CompletedProcess:
        """Run a beads command."""
        return subprocess.run(
            ["bd", *args],
            capture_output=True,
            text=True,
            check=check,
        )

    def _emit_event(self, event_type: str, extra: dict | None = None) -> None:
        """Emit a tambour event."""
        try:
            cmd = [
                sys.executable, "-m", "tambour", "events", "emit", event_type,
                "--issue", self.issue_id,
                "--worktree", str(self.worktree_path),
                "--main-repo", str(self.main_repo),
                "--beads-db", str(self.main_repo / ".beads"),
            ]
            if extra:
                for key, value in extra.items():
                    cmd.extend(["--extra", f"{key}={value}"])

            subprocess.run(cmd, capture_output=True, check=False)
        except Exception:
            pass  # Events are best-effort

    def _get_issue_info(self) -> tuple[str, str, str]:
        """Get issue title, type, and status from beads.

        Returns:
            Tuple of (title, issue_type, status).
        """
        try:
            result = self._run_bd("show", self.issue_id, "--json")
            data = json.loads(result.stdout)
            if data:
                issue = data[0]
                return (
                    issue.get("title", "Unknown"),
                    issue.get("issue_type", "unknown"),
                    issue.get("status", "unknown"),
                )
        except (subprocess.CalledProcessError, json.JSONDecodeError, IndexError, Exception):
            pass
        return ("Unknown", "unknown", "unknown")

    def _get_epic_status(self) -> list[dict]:
        """Get current epic status for detecting auto-close eligibility."""
        try:
            result = self._run_bd("epic", "status", "--json", check=False)
            if result.returncode == 0:
                return json.loads(result.stdout)
        except (json.JSONDecodeError, subprocess.CalledProcessError):
            pass
        return []

    def _branch_exists(self) -> bool:
        """Check if the branch exists locally."""
        result = self._run_git("show-ref", "--verify", "--quiet", f"refs/heads/{self.branch_name}", check=False)
        return result.returncode == 0

    def _worktree_exists(self) -> bool:
        """Check if the worktree directory exists."""
        return self.worktree_path.is_dir()

    def _detach_worktree_head(self) -> bool:
        """Detach HEAD in worktree so branch can be deleted."""
        if not self._worktree_exists():
            return True
        try:
            self._run_git("checkout", "--detach", cwd=self.worktree_path, check=False)
            return True
        except Exception:
            return False

    def run(self) -> FinishResult:
        """Execute the finish workflow.

        Returns:
            FinishResult with the outcome of the operation.
        """
        # Validate worktree exists
        if not self._worktree_exists():
            return FinishResult(
                success=False,
                issue_id=self.issue_id,
                error=f"Worktree not found: {self.worktree_path}",
            )

        # Get issue info
        issue_title, issue_type, issue_status = self._get_issue_info()
        result = FinishResult(
            success=True,
            issue_id=self.issue_id,
            issue_title=issue_title,
        )

        if not self.merge:
            # Just report the worktree location
            print(f"Worktree preserved at: {self.worktree_path}")
            print()
            print("To merge and cleanup later, run:")
            print(f"  tambour finish {self.issue_id} --merge")
            return result

        print(f"=== Finishing agent work for: {self.issue_id} ===")
        print(f"Merging {self.branch_name} into main...")

        # Acquire merge lock
        self._lock = MergeLock(self.main_repo)
        print("Acquiring merge lock...")
        if not self._lock.acquire(self.issue_id):
            return FinishResult(
                success=False,
                issue_id=self.issue_id,
                issue_title=issue_title,
                error=f"Timeout waiting for merge lock after {self._lock.timeout}s. "
                      "Use 'tambour lock-status' to check, or 'tambour lock-release' to force-release.",
            )
        print(f"Acquired merge lock for {self.issue_id}")

        try:
            # Ensure we're on main
            print("Switching to main...")
            self._run_git("checkout", "main")

            # Pull latest
            print("Pulling latest main...")
            pull_result = self._run_git("pull", "origin", "main", "--ff-only", check=False)
            if pull_result.returncode != 0:
                return FinishResult(
                    success=False,
                    issue_id=self.issue_id,
                    issue_title=issue_title,
                    error="Could not fast-forward main. Manual intervention may be needed.",
                )

            # Check if branch exists
            if self._branch_exists():
                # Merge the branch
                print(f"Merging {self.branch_name}...")
                merge_result = self._run_git("merge", self.branch_name, "--no-edit", check=False)
                if merge_result.returncode != 0:
                    return FinishResult(
                        success=False,
                        issue_id=self.issue_id,
                        issue_title=issue_title,
                        error=f"Merge failed: {merge_result.stderr}",
                    )
                result.merged = True

                # Push to origin
                print("Pushing to origin...")
                push_result = self._run_git("push", "origin", "main", check=False)
                if push_result.returncode != 0:
                    return FinishResult(
                        success=False,
                        issue_id=self.issue_id,
                        issue_title=issue_title,
                        merged=True,
                        error=f"Push failed: {push_result.stderr}. The merge lock is still held.",
                    )

                # Emit branch.merged event
                self._emit_event("branch.merged")
            else:
                print(f"Branch {self.branch_name} does not exist. Assuming changes already merged.")
                result.merged = True

        finally:
            # Always release lock
            if self._lock and self._lock.is_acquired:
                print(f"Released merge lock for {self.issue_id}")
                self._lock.release(self.issue_id)

        # Detach HEAD in worktree before removing
        print("Detaching HEAD in worktree...")
        self._detach_worktree_head()

        # Remove worktree
        print("Removing worktree...")
        remove_result = self._run_bd("worktree", "remove", str(self.worktree_path), check=False)
        result.worktree_removed = remove_result.returncode == 0
        if not result.worktree_removed:
            print(f"Warning: Could not remove worktree (may need manual cleanup)")

        # Delete branch
        print("Deleting branch...")
        if self._branch_exists():
            delete_result = self._run_git("branch", "-d", self.branch_name, check=False)
            if delete_result.returncode != 0:
                # Try force delete since changes are merged
                print("Standard delete failed, trying force delete (branch is merged)...")
                delete_result = self._run_git("branch", "-D", self.branch_name, check=False)
            result.branch_deleted = delete_result.returncode == 0
        else:
            print(f"Branch {self.branch_name} already deleted.")
            result.branch_deleted = True

        # Capture epic state before closing
        epics_before = self._get_epic_status()

        # Close issue
        print("Closing issue...")
        if issue_status in ("closed", "done"):
            print(f"Issue {self.issue_id} is already closed.")
            result.issue_closed = True
        else:
            close_result = self._run_bd("close", self.issue_id, check=False)
            result.issue_closed = close_result.returncode == 0
            if not result.issue_closed:
                print(f"Warning: Could not close issue {self.issue_id}")

        # Emit task.completed event
        self._emit_event("task.completed")

        # Check for epics that became eligible for closure
        epics_after = self._get_epic_status()
        result.closed_epics = self._auto_close_epics(epics_before, epics_after)

        print()
        if result.worktree_removed:
            print("Done! Branch merged and worktree cleaned up.")
        else:
            print("Done! Branch merged, issue closed, but worktree needs manual cleanup.")

        return result

    def _auto_close_epics(
        self,
        epics_before: list[dict],
        epics_after: list[dict],
    ) -> list[tuple[str, str]]:
        """Auto-close epics that became eligible after task completion.

        Returns:
            List of (epic_id, epic_title) tuples for closed epics.
        """
        # Find epics eligible before
        eligible_before = {
            e["epic"]["id"]
            for e in epics_before
            if e.get("eligible_for_close")
        }

        # Find newly eligible epics
        closed_epics = []
        for epic_data in epics_after:
            if not epic_data.get("eligible_for_close"):
                continue
            epic_id = epic_data["epic"]["id"]
            if epic_id in eligible_before:
                continue  # Already eligible before

            # Auto-close this epic
            epic_title = epic_data["epic"].get("title", "Unknown")
            print(f"  → Auto-closing completed epic: {epic_id} \"{epic_title}\"")
            close_result = self._run_bd("close", epic_id, check=False)
            if close_result.returncode == 0:
                closed_epics.append((epic_id, epic_title))

        return closed_epics

    def show_completion_summary(self, result: FinishResult) -> None:
        """Display a completion summary.

        Args:
            result: The FinishResult from the run.
        """
        print()
        print("=== Completion Summary ===")
        print(f"✓ Task: {result.issue_id} \"{result.issue_title}\"")

        if result.closed_epics:
            print()
            print("Epics completed:")
            for epic_id, epic_title in result.closed_epics:
                print(f"  ✓ {epic_id} \"{epic_title}\" (all children done)")

    def offer_continuation(self, result: FinishResult) -> bool:
        """Offer to continue to the next task.

        Args:
            result: The FinishResult from the run.

        Returns:
            True if user chose to continue, False otherwise.
        """
        if self.no_continue or not sys.stdin.isatty():
            return False

        self.show_completion_summary(result)

        print()

        # Check for ready tasks
        try:
            ready_result = self._run_bd("ready", "--json")
            ready_data = json.loads(ready_result.stdout)
            ready_tasks = [t for t in ready_data if t.get("issue_type") == "task"]
            ready_count = len(ready_tasks)
        except (subprocess.CalledProcessError, json.JSONDecodeError):
            ready_count = 0
            ready_tasks = []

        if ready_count == 0:
            print("📭 No more ready tasks in the queue!")
            return False

        next_task = ready_tasks[0]
        next_id = next_task.get("id", "unknown")
        next_title = next_task.get("title", "Unknown")

        print(f"📋 {ready_count} ready task(s) remaining")
        print(f"   Next: {next_id} \"{next_title}\"")
        print()
        print("Continue to next task? (y/n)")

        try:
            response = input().strip().lower()
            return response in ("y", "yes")
        except (EOFError, KeyboardInterrupt):
            return False


def cmd_finish(args) -> int:
    """Handle 'finish' command.

    Args:
        args: Parsed command line arguments.

    Returns:
        Exit code (0 for success, non-zero for failure).
    """
    from tambour.config import Config

    # Find the main repo
    main_repo = _find_main_repo()
    if not main_repo:
        print("Error: Could not find main repository", file=sys.stderr)
        return 1

    config = Config.load_or_default()

    finish = FinishCommand(
        issue_id=args.issue,
        main_repo=main_repo,
        merge=args.merge,
        no_continue=args.no_continue,
        config=config,
    )

    result = finish.run()

    if not result.success:
        print(f"Error: {result.error}", file=sys.stderr)
        return 1

    if args.merge and not args.no_continue:
        if finish.offer_continuation(result):
            # Start next agent
            print()
            print("Starting next agent...")
            os.chdir(main_repo)
            start_script = main_repo / "scripts" / "start-agent.sh"
            if start_script.exists():
                os.execv(str(start_script), [str(start_script)])
            else:
                print("Warning: start-agent.sh not found", file=sys.stderr)

    return 0


def cmd_lock_status(args) -> int:
    """Handle 'lock-status' command."""
    repo = _find_current_repo()
    if not repo:
        print("Error: Not in a git repository", file=sys.stderr)
        return 1

    lock = MergeLock(repo)
    status = lock.status()

    if not status.held:
        print("Lock: FREE (no lock held)")
    else:
        print("Lock: HELD")
        if status.metadata:
            print(f"  Holder: {status.metadata.holder}")
            print(f"  Acquired: {status.metadata.acquired_at.isoformat()}")
            print(f"  Host: {status.metadata.host}")
            print(f"  PID: {status.metadata.pid}")

    return 0


def cmd_lock_release(args) -> int:
    """Handle 'lock-release' command."""
    repo = _find_current_repo()
    if not repo:
        print("Error: Not in a git repository", file=sys.stderr)
        return 1

    print("Force-releasing merge lock...")
    lock = MergeLock(repo)
    if lock.force_release():
        print("Lock released.")
    else:
        print("Lock was not held.")

    return 0


def _find_main_repo() -> Path | None:
    """Find the main repository path.

    Handles being called from within a worktree or the main repo.

    Returns:
        Path to the main repository, or None if not found.
    """
    try:
        # Get git toplevel
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        )
        git_toplevel = Path(result.stdout.strip())

        # Check if this is a worktree (.git is a file, not directory)
        git_path = git_toplevel / ".git"
        if git_path.is_file():
            # This is a worktree - parse .git file to find main repo
            content = git_path.read_text().strip()
            if content.startswith("gitdir:"):
                # Format: gitdir: /path/to/main/.git/worktrees/branch-name
                gitdir = content.split(":", 1)[1].strip()
                # Remove /worktrees/... suffix to get main .git dir
                if "/worktrees/" in gitdir:
                    main_git_dir = gitdir.split("/worktrees/")[0]
                    return Path(main_git_dir).parent

        return git_toplevel

    except subprocess.CalledProcessError:
        return None


def _find_current_repo() -> Path | None:
    """Find the current git repository path.

    Returns the git toplevel of the current directory, whether it's
    the main repo or a worktree. Unlike _find_main_repo(), this doesn't
    traverse to the main repo from a worktree.

    This is useful for operations like lock management that work from
    any repo sharing the same remote.

    Returns:
        Path to the current git repository, or None if not in a git repo.
    """
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        )
        return Path(result.stdout.strip())
    except subprocess.CalledProcessError:
        return None
