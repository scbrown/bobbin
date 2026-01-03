"""Tests for event dispatching."""

import threading
import time
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from tambour.config import Config, PluginConfig
from tambour.events import Event, EventDispatcher, EventType, PluginResult


@pytest.fixture
def mock_config():
    """Create a mock configuration."""
    config = Config()
    
    # Plugin 1: Blocking
    p1 = PluginConfig(
        name="p1-blocking",
        on="branch.merged",
        run="echo 'blocking'",
        blocking=True
    )
    
    # Plugin 2: Non-blocking
    p2 = PluginConfig(
        name="p2-async",
        on="branch.merged",
        run="echo 'async'",
        blocking=False
    )
    
    config.plugins = {"p1": p1, "p2": p2}
    return config


def test_dispatch_mixed_blocking(mock_config, tmp_path):
    """Test dispatching with mixed blocking and non-blocking plugins."""
    log_file = tmp_path / "events.log"
    dispatcher = EventDispatcher(mock_config, log_file=log_file)
    event = Event(event_type=EventType.BRANCH_MERGED)

    # Mock subprocess.run
    with patch("subprocess.run") as mock_run:
        def side_effect(*args, **kwargs):
            return MagicMock(returncode=0, stdout="done", stderr="")
        
        mock_run.side_effect = side_effect
        
        results = dispatcher.dispatch(event)

        # Wait for threads to finish
        for thread in threading.enumerate():
            if thread is not threading.current_thread():
                thread.join(timeout=1.0)

    # Check immediate results
    assert len(results) == 2
    
    # p1 (blocking) should have real output
    p1_res = next(r for r in results if r.plugin_name == "p1-blocking")
    assert p1_res.success
    assert p1_res.output == "done"
    
    # p2 (async) should have placeholder output
    p2_res = next(r for r in results if r.plugin_name == "p2-async")
    assert p2_res.success
    assert "Async execution started" in p2_res.output

    # Check log file
    content = log_file.read_text()
    assert "Plugin 'p1-blocking'" in content
    assert "Plugin 'p2-async'" in content
    assert "SUCCESS" in content


def test_blocking_failure_stops_chain(mock_config, tmp_path):
    """Test that a blocking plugin failure stops the chain."""
    # Make p1 fail
    mock_config.plugins["p1"].run = "exit 1"
    
    log_file = tmp_path / "events.log"
    dispatcher = EventDispatcher(mock_config, log_file=log_file)
    event = Event(event_type=EventType.BRANCH_MERGED)

    with patch("subprocess.run") as mock_run:
        # p1 fails
        mock_run.return_value = MagicMock(returncode=1, stdout="", stderr="error")
        
        results = dispatcher.dispatch(event)

    # Should only have p1 result
    assert len(results) == 1
    assert results[0].plugin_name == "p1-blocking"
    assert not results[0].success


def test_event_env_vars():
    """Test that event is converted to environment variables correctly."""
    event = Event(
        event_type=EventType.BRANCH_MERGED,
        issue_id="issue-123",
        extra={"custom": "value"}
    )
    
    env = event.to_env()
    
    assert env["TAMBOUR_EVENT"] == "branch.merged"
    assert env["TAMBOUR_ISSUE_ID"] == "issue-123"
    assert env["TAMBOUR_CUSTOM"] == "value"
    assert "TAMBOUR_TIMESTAMP" in env
