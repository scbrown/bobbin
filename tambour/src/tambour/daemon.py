"""Daemon implementation for tambour.

Provides background process management for health monitoring
and event dispatch.
"""

from __future__ import annotations

import os
import signal
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config

# Default paths
DEFAULT_PID_FILE = Path.home() / ".tambour" / "daemon.pid"
DEFAULT_LOG_FILE = Path.home() / ".tambour" / "daemon.log"


class Daemon:
    """Tambour daemon for background operations.

    The daemon provides:
    - Periodic health checks for zombie tasks
    - Event dispatch to plugins
    - Worktree monitoring
    """

    def __init__(
        self,
        pid_file: Path | None = None,
        log_file: Path | None = None,
    ):
        """Initialize the daemon.

        Args:
            pid_file: Path to PID file. Defaults to ~/.tambour/daemon.pid
            log_file: Path to log file. Defaults to ~/.tambour/daemon.log
        """
        self.pid_file = pid_file or DEFAULT_PID_FILE
        self.log_file = log_file or DEFAULT_LOG_FILE
        self._running = False

    def start(self) -> int:
        """Start the daemon.

        Returns:
            Exit code (0 for success, non-zero for failure).
        """
        # Ensure directory exists
        self.pid_file.parent.mkdir(parents=True, exist_ok=True)
        self.log_file.parent.mkdir(parents=True, exist_ok=True)

        # Check if already running
        if self._is_running():
            pid = self._read_pid()
            print(f"Daemon already running (PID: {pid})", file=sys.stderr)
            return 1

        print("Starting tambour daemon...")
        print(f"  PID file: {self.pid_file}")
        print(f"  Log file: {self.log_file}")

        # For now, just write a stub message
        # Full daemonization will be implemented in a later phase
        print("")
        print("Note: Full daemon implementation is a stub.")
        print("The daemon will be fully implemented in a later phase.")
        print("For now, use the shell scripts for health monitoring.")

        return 0

    def stop(self) -> int:
        """Stop the daemon.

        Returns:
            Exit code (0 for success, non-zero for failure).
        """
        if not self._is_running():
            print("Daemon is not running")
            return 0

        pid = self._read_pid()
        if pid is None:
            print("Could not read daemon PID", file=sys.stderr)
            return 1

        print(f"Stopping daemon (PID: {pid})...")

        try:
            os.kill(pid, signal.SIGTERM)
            # Wait for process to exit
            for _ in range(10):
                time.sleep(0.5)
                try:
                    os.kill(pid, 0)  # Check if still running
                except ProcessLookupError:
                    break
            else:
                # Force kill if still running
                os.kill(pid, signal.SIGKILL)

            self.pid_file.unlink(missing_ok=True)
            print("Daemon stopped")
            return 0

        except ProcessLookupError:
            # Process already gone
            self.pid_file.unlink(missing_ok=True)
            print("Daemon was not running (stale PID file removed)")
            return 0

        except PermissionError:
            print(f"Permission denied to stop daemon (PID: {pid})", file=sys.stderr)
            return 1

    def status(self) -> int:
        """Show daemon status.

        Returns:
            Exit code (0 if running, 1 if not running).
        """
        if self._is_running():
            pid = self._read_pid()
            print(f"Daemon is running (PID: {pid})")
            return 0
        else:
            print("Daemon is not running")
            return 1

    def _is_running(self) -> bool:
        """Check if the daemon is currently running."""
        pid = self._read_pid()
        if pid is None:
            return False

        try:
            os.kill(pid, 0)  # Signal 0 just checks if process exists
            return True
        except (ProcessLookupError, PermissionError):
            return False

    def _read_pid(self) -> int | None:
        """Read the PID from the PID file."""
        if not self.pid_file.exists():
            return None

        try:
            return int(self.pid_file.read_text().strip())
        except (ValueError, OSError):
            return None

    def _write_pid(self) -> None:
        """Write the current PID to the PID file."""
        self.pid_file.write_text(str(os.getpid()))

    def _run_loop(self, config: Config) -> None:
        """Main daemon loop.

        Args:
            config: The tambour configuration.
        """
        from tambour.health import HealthChecker

        self._running = True
        checker = HealthChecker(config)

        # Set up signal handlers
        signal.signal(signal.SIGTERM, self._handle_signal)
        signal.signal(signal.SIGINT, self._handle_signal)

        while self._running:
            try:
                checker.check_all()
                time.sleep(config.daemon.health_interval)
            except Exception as e:
                self._log(f"Error in health check: {e}")

    def _handle_signal(self, signum: int, frame: object) -> None:
        """Handle termination signals."""
        self._running = False

    def _log(self, message: str) -> None:
        """Write a message to the log file."""
        from datetime import datetime, timezone

        timestamp = datetime.now(timezone.utc).isoformat()
        with open(self.log_file, "a") as f:
            f.write(f"[{timestamp}] {message}\n")
