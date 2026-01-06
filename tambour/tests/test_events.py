"""Tests for event dispatching."""

import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from tambour.config import Config, PluginConfig
from tambour.events import (
    Event,
    EventDispatcher,
    EventType,
    PluginResult,
    SessionEvent,
    ToolEvent,
)


@pytest.fixture
def mock_config():
    """Create a mock configuration."""
    config = Config()

    # Plugin 1: Blocking
    p1 = PluginConfig(
        name="p1-blocking",
        on=["branch.merged"],
        run="echo 'blocking'",
        blocking=True,
    )

    # Plugin 2: Non-blocking
    p2 = PluginConfig(
        name="p2-async",
        on=["branch.merged"],
        run="echo 'async'",
        blocking=False,
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


def test_event_env_vars_with_paths():
    """Test that path variables are correctly set in environment."""
    event = Event(
        event_type=EventType.BRANCH_MERGED,
        main_repo=Path("/path/to/repo"),
        beads_db=Path("/path/to/beads"),
    )

    env = event.to_env()

    assert env["TAMBOUR_MAIN_REPO"] == "/path/to/repo"
    assert env["TAMBOUR_BEADS_DB"] == "/path/to/beads"


# Tests for new tool and session event types


class TestToolEvent:
    """Tests for ToolEvent dataclass."""

    def test_tool_event_creation(self):
        """Test creating a ToolEvent with required fields."""
        tool_event = ToolEvent(
            tool_name="Read",
            tool_input={"file_path": "/path/to/file.py"},
            tool_response={"success": "true", "lines": "100"},
            session_id="sess_abc123",
        )

        assert tool_event.tool_name == "Read"
        assert tool_event.tool_input == {"file_path": "/path/to/file.py"}
        assert tool_event.tool_response == {"success": "true", "lines": "100"}
        assert tool_event.session_id == "sess_abc123"
        assert tool_event.issue_id is None
        assert tool_event.worktree is None
        assert tool_event.duration_ms is None
        assert tool_event.error is None

    def test_tool_event_with_optional_fields(self):
        """Test creating a ToolEvent with all optional fields."""
        tool_event = ToolEvent(
            tool_name="Edit",
            tool_input={"file_path": "/path/to/file.py", "old_string": "foo"},
            tool_response={"success": "true"},
            session_id="sess_abc123",
            issue_id="bobbin-xyz",
            worktree=Path("/path/to/worktree"),
            duration_ms=45,
            error=None,
        )

        assert tool_event.issue_id == "bobbin-xyz"
        assert tool_event.worktree == Path("/path/to/worktree")
        assert tool_event.duration_ms == 45

    def test_tool_event_to_event_success(self):
        """Test converting ToolEvent to Event for successful tool use."""
        tool_event = ToolEvent(
            tool_name="Read",
            tool_input={"file_path": "/path/to/file.py"},
            tool_response={"success": "true"},
            session_id="sess_abc123",
            issue_id="bobbin-xyz",
            duration_ms=50,
        )

        event = tool_event.to_event(failed=False)

        assert event.event_type == EventType.TOOL_USED
        assert event.issue_id == "bobbin-xyz"
        env = event.to_env()
        assert env["TAMBOUR_EVENT"] == "tool.used"
        assert env["TAMBOUR_TOOL_NAME"] == "Read"
        assert env["TAMBOUR_SESSION_ID"] == "sess_abc123"
        assert env["TAMBOUR_DURATION_MS"] == "50"

    def test_tool_event_to_event_failure(self):
        """Test converting ToolEvent to Event for failed tool use."""
        tool_event = ToolEvent(
            tool_name="Edit",
            tool_input={"file_path": "/path/to/file.py"},
            tool_response={"success": "false"},
            session_id="sess_abc123",
            error="old_string not found",
        )

        event = tool_event.to_event(failed=True)

        assert event.event_type == EventType.TOOL_FAILED
        env = event.to_env()
        assert env["TAMBOUR_EVENT"] == "tool.failed"
        assert env["TAMBOUR_TOOL_NAME"] == "Edit"
        assert env["TAMBOUR_ERROR"] == "old_string not found"


class TestSessionEvent:
    """Tests for SessionEvent dataclass."""

    def test_session_event_creation(self):
        """Test creating a SessionEvent with required fields."""
        session_event = SessionEvent(session_id="sess_abc123")

        assert session_event.session_id == "sess_abc123"
        assert session_event.issue_id is None
        assert session_event.worktree is None
        assert session_event.file_path is None
        assert session_event.lines is None
        assert session_event.success is True

    def test_session_started_event(self):
        """Test creating a session.started event."""
        session_event = SessionEvent(
            session_id="sess_abc123",
            issue_id="bobbin-xyz",
            worktree=Path("/path/to/worktree"),
        )

        event = session_event.to_event(EventType.SESSION_STARTED)

        assert event.event_type == EventType.SESSION_STARTED
        env = event.to_env()
        assert env["TAMBOUR_EVENT"] == "session.started"
        assert env["TAMBOUR_SESSION_ID"] == "sess_abc123"
        assert env["TAMBOUR_SUCCESS"] == "true"

    def test_session_file_read_event(self):
        """Test creating a session.file_read event."""
        session_event = SessionEvent(
            session_id="sess_abc123",
            file_path=Path("/path/to/file.py"),
            lines=150,
            success=True,
        )

        event = session_event.to_event(EventType.SESSION_FILE_READ)

        assert event.event_type == EventType.SESSION_FILE_READ
        env = event.to_env()
        assert env["TAMBOUR_EVENT"] == "session.file_read"
        assert env["TAMBOUR_FILE_PATH"] == "/path/to/file.py"
        assert env["TAMBOUR_LINES"] == "150"
        assert env["TAMBOUR_SUCCESS"] == "true"

    def test_session_file_written_event(self):
        """Test creating a session.file_written event."""
        session_event = SessionEvent(
            session_id="sess_abc123",
            file_path=Path("/path/to/file.py"),
            success=True,
        )

        event = session_event.to_event(EventType.SESSION_FILE_WRITTEN)

        assert event.event_type == EventType.SESSION_FILE_WRITTEN
        env = event.to_env()
        assert env["TAMBOUR_EVENT"] == "session.file_written"
        assert env["TAMBOUR_FILE_PATH"] == "/path/to/file.py"


class TestNewEventTypes:
    """Tests for new EventType enum values."""

    def test_tool_event_types_exist(self):
        """Test that tool.* event types are defined."""
        assert EventType.TOOL_USED.value == "tool.used"
        assert EventType.TOOL_FAILED.value == "tool.failed"

    def test_session_event_types_exist(self):
        """Test that session.* event types are defined."""
        assert EventType.SESSION_STARTED.value == "session.started"
        assert EventType.SESSION_FILE_READ.value == "session.file_read"
        assert EventType.SESSION_FILE_WRITTEN.value == "session.file_written"

    def test_tool_events_dispatch_to_multi_event_plugin(self):
        """Test that tool events dispatch to plugins subscribed to multiple events."""
        config = Config()

        # Plugin that subscribes to both tool.used and tool.failed
        metrics_plugin = PluginConfig(
            name="metrics-collector",
            on=["tool.used", "tool.failed"],
            run="collect-metrics.sh",
            blocking=False,
        )
        config.plugins = {"metrics": metrics_plugin}

        # Should match both event types
        used_plugins = config.get_plugins_for_event("tool.used")
        assert len(used_plugins) == 1
        assert used_plugins[0].name == "metrics-collector"

        failed_plugins = config.get_plugins_for_event("tool.failed")
        assert len(failed_plugins) == 1
        assert failed_plugins[0].name == "metrics-collector"

        # Should not match unsubscribed events
        other_plugins = config.get_plugins_for_event("branch.merged")
        assert len(other_plugins) == 0

