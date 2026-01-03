"""Tests for heartbeat mechanism."""

import json
import time
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest
from tambour.heartbeat import HeartbeatWriter


def test_heartbeat_writer_initialization(tmp_path: Path):
    """Test HeartbeatWriter initialization."""
    writer = HeartbeatWriter(tmp_path, interval=1)
    assert writer.worktree_path == tmp_path
    assert writer.interval == 1
    assert writer.heartbeat_file == tmp_path / ".tambour" / "heartbeat"


def test_write_heartbeat(tmp_path: Path):
    """Test writing a heartbeat file."""
    writer = HeartbeatWriter(tmp_path, interval=1)
    
    # Create directory (HeartbeatWriter.start does this, but we test _write_heartbeat directly)
    (tmp_path / ".tambour").mkdir()
    
    writer._write_heartbeat()
    
    assert writer.heartbeat_file.exists()
    
    data = json.loads(writer.heartbeat_file.read_text())
    assert "timestamp" in data
    assert "pid" in data
    assert isinstance(data["pid"], int)


@patch("tambour.heartbeat.time.sleep")
def test_start_loop(mock_sleep: MagicMock, tmp_path: Path):
    """Test the start loop (run once and stop)."""
    writer = HeartbeatWriter(tmp_path, interval=1)
    
    # Mock _write_heartbeat to stop the loop after one call
    original_write = writer._write_heartbeat
    
    def side_effect():
        original_write()
        writer._running = False
        
    # We also need to patch sys.exit to avoid exiting the test
    with patch.object(writer, "_write_heartbeat", side_effect=side_effect) as mock_write:
        with patch("sys.exit"):
            # Patch unlink to prevent deletion so we can verify existence
            with patch("pathlib.Path.unlink") as mock_unlink:
                writer.start()
            
    assert writer.heartbeat_file.exists()
    mock_write.assert_called_once()
    mock_unlink.assert_called_once()
    # Should have slept once
    mock_sleep.assert_called_once_with(1)
