"""Tests for plugin configuration parsing."""

import tempfile
from pathlib import Path

import pytest

from tambour.config import (
    Config,
    ContextProviderConfig,
    DaemonConfig,
    PluginConfig,
    WorktreeConfig,
    VALID_EVENT_NAMES,
)


class TestPluginConfig:
    """Tests for PluginConfig parsing."""

    def test_from_dict_with_required_fields_only(self):
        """Test parsing plugin with only required fields."""
        data = {"on": "branch.merged", "run": "bobbin index"}
        plugin = PluginConfig.from_dict("indexer", data)

        assert plugin.name == "indexer"
        assert plugin.on == ["branch.merged"]
        assert plugin.run == "bobbin index"
        # Check defaults
        assert plugin.blocking is False
        assert plugin.timeout == 30
        assert plugin.enabled is True

    def test_from_dict_with_all_fields(self):
        """Test parsing plugin with all fields specified."""
        data = {
            "on": "agent.finished",
            "run": "echo done",
            "blocking": True,
            "timeout": 60,
            "enabled": False,
        }
        plugin = PluginConfig.from_dict("notify", data)

        assert plugin.name == "notify"
        assert plugin.on == ["agent.finished"]
        assert plugin.run == "echo done"
        assert plugin.blocking is True
        assert plugin.timeout == 60
        assert plugin.enabled is False

    def test_from_dict_with_multiple_events(self):
        """Test parsing plugin that subscribes to multiple events."""
        data = {
            "on": ["tool.used", "tool.failed"],
            "run": "metrics-collector.sh",
        }
        plugin = PluginConfig.from_dict("metrics", data)

        assert plugin.name == "metrics"
        assert plugin.on == ["tool.used", "tool.failed"]
        assert plugin.run == "metrics-collector.sh"
        assert plugin.matches_event("tool.used")
        assert plugin.matches_event("tool.failed")
        assert not plugin.matches_event("agent.finished")

    def test_from_dict_missing_on_field(self):
        """Test error when 'on' field is missing."""
        data = {"run": "echo hello"}
        with pytest.raises(ValueError) as exc_info:
            PluginConfig.from_dict("broken", data)
        assert "missing required field 'on'" in str(exc_info.value)
        assert "broken" in str(exc_info.value)

    def test_from_dict_missing_run_field(self):
        """Test error when 'run' field is missing."""
        data = {"on": "branch.merged"}
        with pytest.raises(ValueError) as exc_info:
            PluginConfig.from_dict("broken", data)
        assert "missing required field 'run'" in str(exc_info.value)
        assert "broken" in str(exc_info.value)

    def test_from_dict_invalid_event_name(self):
        """Test error when event name is invalid."""
        data = {"on": "invalid.event", "run": "echo hello"}
        with pytest.raises(ValueError) as exc_info:
            PluginConfig.from_dict("broken", data)
        assert "specifies invalid event 'invalid.event'" in str(exc_info.value)
        assert "broken" in str(exc_info.value)
        # Check that valid events are listed in error
        assert "agent.spawned" in str(exc_info.value)

    def test_all_valid_event_names_accepted(self):
        """Test that all valid event names are accepted."""
        for event_name in VALID_EVENT_NAMES:
            data = {"on": event_name, "run": "echo test"}
            plugin = PluginConfig.from_dict(f"plugin_{event_name}", data)
            assert plugin.on == [event_name]
            assert plugin.matches_event(event_name)


class TestContextProviderConfig:
    """Tests for ContextProviderConfig parsing."""

    def test_from_dict_with_required_fields_only(self):
        """Test parsing provider with only required fields."""
        data = {"run": "tree.sh"}
        provider = ContextProviderConfig.from_dict("tree", data)

        assert provider.name == "tree"
        assert provider.run == "tree.sh"
        # Check defaults
        assert provider.timeout == 10
        assert provider.enabled is True
        assert provider.order == 100
        assert provider.options == {}

    def test_from_dict_with_options(self):
        """Test parsing provider with extra options."""
        data = {
            "run": "tree.sh",
            "exclude": [".git", "node_modules"],
            "depth": 3,
            "timeout": 20,
        }
        provider = ContextProviderConfig.from_dict("tree", data)

        assert provider.name == "tree"
        assert provider.run == "tree.sh"
        assert provider.timeout == 20
        assert provider.options == {
            "exclude": [".git", "node_modules"],
            "depth": 3,
        }

    def test_from_dict_missing_run_field(self):
        """Test error when 'run' field is missing."""
        data = {"exclude": []}
        with pytest.raises(ValueError) as exc_info:
            ContextProviderConfig.from_dict("broken", data)
        assert "missing required field 'run'" in str(exc_info.value)


class TestConfig:
    """Tests for Config parsing."""

    def test_load_from_toml_string(self):
        """Test loading config from TOML content."""
        toml_content = """
[tambour]
version = "1"

[daemon]
health_interval = 120
zombie_threshold = 600
auto_recover = true

[worktree]
base_path = "../custom-worktrees"

[plugins.indexer]
on = "branch.merged"
run = "bobbin index"
blocking = false
timeout = 60
enabled = true

[plugins.notifier]
on = "agent.finished"
run = "notify-send 'Agent done'"
"""
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".toml", delete=False
        ) as f:
            f.write(toml_content)
            config_path = Path(f.name)

        try:
            config = Config.load(config_path)

            assert config.version == "1"
            assert config.daemon.health_interval == 120
            assert config.daemon.zombie_threshold == 600
            assert config.daemon.auto_recover is True
            assert config.worktree.base_path == "../custom-worktrees"
            assert len(config.plugins) == 2
            assert "indexer" in config.plugins
            assert "notifier" in config.plugins
            assert config.plugins["indexer"].on == ["branch.merged"]
            assert config.plugins["notifier"].on == ["agent.finished"]
        finally:
            config_path.unlink()

    def test_load_with_defaults(self):
        """Test that defaults are applied for missing sections."""
        toml_content = """
[tambour]
version = "1"

[plugins.minimal]
on = "task.claimed"
run = "echo claimed"
"""
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".toml", delete=False
        ) as f:
            f.write(toml_content)
            config_path = Path(f.name)

        try:
            config = Config.load(config_path)

            # Check defaults
            assert config.daemon.health_interval == 60
            assert config.daemon.zombie_threshold == 300
            assert config.daemon.auto_recover is False
            assert config.worktree.base_path == "../{repo}-worktrees"
            # Plugin should be there
            assert "minimal" in config.plugins
        finally:
            config_path.unlink()

    def test_load_or_default_with_missing_file(self):
        """Test load_or_default returns default when file doesn't exist."""
        config = Config.load_or_default(Path("/nonexistent/config.toml"))

        assert config.version == "1"
        assert len(config.plugins) == 0
        assert config.daemon.health_interval == 60

    def test_load_missing_file_raises_error(self):
        """Test load raises FileNotFoundError for missing file."""
        with pytest.raises(FileNotFoundError):
            Config.load(Path("/nonexistent/config.toml"))

    def test_load_with_invalid_plugin_event(self):
        """Test that invalid plugin event names are rejected."""
        toml_content = """
[plugins.broken]
on = "invalid.event"
run = "echo broken"
"""
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".toml", delete=False
        ) as f:
            f.write(toml_content)
            config_path = Path(f.name)

        try:
            with pytest.raises(ValueError) as exc_info:
                Config.load(config_path)
            assert "specifies invalid event" in str(exc_info.value)
        finally:
            config_path.unlink()

    def test_get_plugins_for_event(self):
        """Test filtering plugins by event type."""
        toml_content = """
[plugins.indexer1]
on = "branch.merged"
run = "bobbin index"

[plugins.indexer2]
on = "branch.merged"
run = "bobbin index --all"

[plugins.notifier]
on = "agent.finished"
run = "notify-send"

[plugins.disabled]
on = "branch.merged"
run = "echo disabled"
enabled = false
"""
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".toml", delete=False
        ) as f:
            f.write(toml_content)
            config_path = Path(f.name)

        try:
            config = Config.load(config_path)

            merge_plugins = config.get_plugins_for_event("branch.merged")
            assert len(merge_plugins) == 2  # disabled one is excluded
            plugin_names = {p.name for p in merge_plugins}
            assert plugin_names == {"indexer1", "indexer2"}

            finish_plugins = config.get_plugins_for_event("agent.finished")
            assert len(finish_plugins) == 1
            assert finish_plugins[0].name == "notifier"

            # No plugins for this event
            zombie_plugins = config.get_plugins_for_event("health.zombie")
            assert len(zombie_plugins) == 0
        finally:
            config_path.unlink()


class TestValidEventNames:
    """Tests for VALID_EVENT_NAMES constant."""

    def test_contains_expected_events(self):
        """Test that all expected events are in the set."""
        expected = {
            # Lifecycle events
            "agent.spawned",
            "agent.finished",
            "branch.merged",
            "task.claimed",
            "task.completed",
            "health.zombie",
            # Tool use events
            "tool.used",
            "tool.failed",
            # Session events
            "session.started",
            "session.file_read",
            "session.file_written",
        }
        assert VALID_EVENT_NAMES == expected

    def test_is_frozenset(self):
        """Test that VALID_EVENT_NAMES is immutable."""
        assert isinstance(VALID_EVENT_NAMES, frozenset)
