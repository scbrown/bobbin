"""Heartbeat mechanism for agents.

Provides a writer that periodically updates a heartbeat file
to indicate liveness.
"""

from __future__ import annotations

import json
import os
import signal
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import NoReturn


class HeartbeatWriter:
    """Writes periodic heartbeats to a file."""

    def __init__(self, worktree_path: Path, interval: int = 30):
        """Initialize the heartbeat writer.

        Args:
            worktree_path: Path to the worktree.
            interval: Interval in seconds between heartbeats.
        """
        self.worktree_path = worktree_path
        self.interval = interval
        self.heartbeat_file = worktree_path / ".tambour" / "heartbeat"
        self._running = False

    def start(self) -> NoReturn:
        """Start the heartbeat loop.

        Runs until interrupted.
        """
        # Ensure .tambour directory exists
        self.heartbeat_file.parent.mkdir(parents=True, exist_ok=True)

        self._running = True
        
        # Handle signals
        signal.signal(signal.SIGTERM, self._stop)
        signal.signal(signal.SIGINT, self._stop)

        print(f"Starting heartbeat writer for {self.worktree_path}")
        print(f"File: {self.heartbeat_file}")

        while self._running:
            try:
                self._write_heartbeat()
                time.sleep(self.interval)
            except Exception as e:
                print(f"Error writing heartbeat: {e}", file=sys.stderr)
                time.sleep(5)  # Retry delay

        # Cleanup on exit
        self.heartbeat_file.unlink(missing_ok=True)
        sys.exit(0)

    def _stop(self, signum, frame):
        """Signal handler to stop the loop."""
        self._running = False

    def _write_heartbeat(self) -> None:
        """Write the current timestamp to the heartbeat file."""
        data = {
            "timestamp": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "pid": os.getpid(),
        }
        
        # Write atomically-ish (write then flush)
        # Using a temp file and rename would be truly atomic but 
        # direct write is sufficient for a timestamp check
        with open(self.heartbeat_file, "w") as f:
            json.dump(data, f)
            f.flush()
            os.fsync(f.fileno())
