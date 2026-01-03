"""Daemon implementation for tambour.

Provides background process management for health monitoring
and event dispatch.
"""

from __future__ import annotations

import logging
import os
import signal
import sys
import time
from logging.handlers import RotatingFileHandler
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config

# Default paths (fallback if local .tambour not found)
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
            pid_file: Path to PID file. Defaults to .tambour/daemon.pid (local) or ~/.tambour/daemon.pid
            log_file: Path to log file. Defaults to .tambour/daemon.log (local) or ~/.tambour/daemon.log
        """
        cwd_tambour = Path.cwd() / ".tambour"
        
        if pid_file:
            self.pid_file = pid_file
        elif cwd_tambour.exists():
            self.pid_file = cwd_tambour / "daemon.pid"
        else:
            self.pid_file = DEFAULT_PID_FILE

        if log_file:
            self.log_file = log_file
        elif cwd_tambour.exists():
            self.log_file = cwd_tambour / "daemon.log"
        else:
            self.log_file = DEFAULT_LOG_FILE

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

        # Resolve config path before daemonizing (while we have correct CWD)
        # We don't load it yet, just resolve the path
        from tambour.config import Config
        try:
            config_path = Config._find_config().resolve()
        except Exception:
            # Fallback if no config found, Config.load_or_default will handle it
            config_path = None

        # Double-fork daemonization
        try:
            pid = os.fork()
            if pid > 0:
                # First parent returns
                return 0
        except OSError as e:
            print(f"fork #1 failed: {e}", file=sys.stderr)
            return 1

        # Decouple from parent environment
        os.setsid()
        os.umask(0)

        # Do second fork
        try:
            pid = os.fork()
            if pid > 0:
                # Second parent exits
                sys.exit(0)
        except OSError as e:
            print(f"fork #2 failed: {e}", file=sys.stderr)
            sys.exit(1)

        # Redirect standard file descriptors
        sys.stdout.flush()
        sys.stderr.flush()
        with open(os.devnull, 'r') as si:
            os.dup2(si.fileno(), sys.stdin.fileno())
        with open(self.log_file, 'a+') as so:
            os.dup2(so.fileno(), sys.stdout.fileno())
            os.dup2(so.fileno(), sys.stderr.fileno())

        # Write PID file
        self._write_pid()

        # Run loop
        try:
            # Load config
            if config_path:
                config = Config.load_or_default(config_path)
            else:
                config = Config.load_or_default()
                
            self._run_loop(config)
        except Exception as e:
            # Last ditch error logging (stderr is redirected to log file)
            print(f"Daemon crashed: {e}", file=sys.stderr)
            sys.exit(1)
        finally:
            self.pid_file.unlink(missing_ok=True)
        
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

        # Setup logging
        handler = RotatingFileHandler(
            self.log_file, maxBytes=10*1024*1024, backupCount=5
        )
        formatter = logging.Formatter(
            '[%(asctime)s] %(levelname)s: %(message)s',
            datefmt='%Y-%m-%dT%H:%M:%S%z'
        )
        handler.setFormatter(formatter)
        
        logger = logging.getLogger("tambour.daemon")
        logger.setLevel(logging.INFO)
        # Avoid adding multiple handlers if re-initialized
        if not logger.handlers:
            logger.addHandler(handler)
        
        logger.info("Daemon started")

        self._running = True
        checker = HealthChecker(config)

        # Set up signal handlers
        def signal_handler(signum, frame):
            logger.info(f"Received signal {signum}, shutting down...")
            self._running = False
        
        signal.signal(signal.SIGTERM, signal_handler)
        signal.signal(signal.SIGINT, signal_handler)

        while self._running:
            try:
                # logger.debug("Running health check...")
                checker.check_all()
                
                # Sleep loop for responsiveness
                for _ in range(config.daemon.health_interval):
                    if not self._running:
                        break
                    time.sleep(1)
            except Exception as e:
                logger.error(f"Error in health check: {e}", exc_info=True)
                time.sleep(5)
        
        logger.info("Daemon stopped")
