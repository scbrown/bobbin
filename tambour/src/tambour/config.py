"""Configuration parsing for tambour.

Parses .tambour/config.toml files for plugin definitions and daemon settings.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# Valid event names that plugins can subscribe to
VALID_EVENT_NAMES = frozenset({
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
})


@dataclass
class PluginConfig:
    """Configuration for a single plugin."""

    name: str
    on: list[str]  # Event types to trigger on (supports multiple events)
    run: str  # Command to execute
    blocking: bool = False
    timeout: int = 30  # seconds
    enabled: bool = True

    def matches_event(self, event_type: str) -> bool:
        """Check if this plugin should trigger for the given event type.

        Args:
            event_type: The event type string (e.g., "tool.used").

        Returns:
            True if the plugin subscribes to this event type.
        """
        return event_type in self.on

    @classmethod
    def from_dict(cls, name: str, data: dict[str, Any]) -> PluginConfig:
        """Create a PluginConfig from a dictionary.

        Args:
            name: The plugin name (from config section).
            data: Dictionary of plugin configuration values.

        Returns:
            A configured PluginConfig instance.

        Raises:
            ValueError: If required fields are missing or event name is invalid.
        """
        if "on" not in data:
            raise ValueError(f"Plugin '{name}' missing required field 'on'")
        if "run" not in data:
            raise ValueError(f"Plugin '{name}' missing required field 'run'")

        # Support both single string and list of strings for 'on'
        on_value = data["on"]
        if isinstance(on_value, str):
            event_names = [on_value]
        elif isinstance(on_value, list):
            event_names = on_value
        else:
            raise ValueError(
                f"Plugin '{name}' has invalid 'on' field: expected string or list"
            )

        # Validate all event names
        for event_name in event_names:
            if event_name not in VALID_EVENT_NAMES:
                raise ValueError(
                    f"Plugin '{name}' specifies invalid event '{event_name}'. "
                    f"Valid events are: {', '.join(sorted(VALID_EVENT_NAMES))}"
                )

        return cls(
            name=name,
            on=event_names,
            run=data["run"],
            blocking=data.get("blocking", False),
            timeout=data.get("timeout", 30),
            enabled=data.get("enabled", True),
        )


@dataclass
class ContextProviderConfig:
    """Configuration for a context provider.

    Context providers generate content that gets injected into the agent prompt
    at session start. Unlike regular plugins that run for side effects, context
    providers return text that becomes part of the agent's initial context.
    """

    name: str
    run: str  # Command to execute (stdout becomes context)
    timeout: int = 10  # seconds (should be fast)
    enabled: bool = True
    order: int = 100  # Lower runs first (for ordering providers)
    options: dict[str, Any] = field(default_factory=dict)  # Extra provider-specific options

    @classmethod
    def from_dict(cls, name: str, data: dict[str, Any]) -> ContextProviderConfig:
        """Create a ContextProviderConfig from a dictionary.

        Args:
            name: The provider name (from config section).
            data: Dictionary of provider configuration values.

        Returns:
            A configured ContextProviderConfig instance.

        Raises:
            ValueError: If required fields are missing.
        """
        if "run" not in data:
            raise ValueError(f"Context provider '{name}' missing required field 'run'")

        # Extract known fields
        run = data["run"]
        timeout = data.get("timeout", 10)
        enabled = data.get("enabled", True)
        order = data.get("order", 100)

        # Collect any remaining fields as options
        known_keys = {"run", "timeout", "enabled", "order"}
        options = {k: v for k, v in data.items() if k not in known_keys}

        return cls(
            name=name,
            run=run,
            timeout=timeout,
            enabled=enabled,
            order=order,
            options=options,
        )


@dataclass
class DaemonConfig:
    """Configuration for the daemon."""

    health_interval: int = 60  # seconds between health checks
    zombie_threshold: int = 300  # seconds before task is zombie
    auto_recover: bool = False  # automatically unclaim zombies


@dataclass
class AgentConfig:
    """Configuration for the agent."""

    default_cli: str = "claude"  # Default CLI command to run


@dataclass
class WorktreeConfig:
    """Configuration for worktree paths."""

    base_path: str = "../{repo}-worktrees"


@dataclass
class Config:
    """Main configuration container."""

    version: str = "1"
    daemon: DaemonConfig = field(default_factory=DaemonConfig)
    agent: AgentConfig = field(default_factory=AgentConfig)
    worktree: WorktreeConfig = field(default_factory=WorktreeConfig)
    plugins: dict[str, PluginConfig] = field(default_factory=dict)
    context_providers: dict[str, ContextProviderConfig] = field(default_factory=dict)
    config_path: Path | None = None

    @classmethod
    def load(cls, path: Path | None = None) -> Config:
        """Load configuration from a file.

        Args:
            path: Path to config file. If None, searches for .tambour/config.toml
                  in current directory and parents.

        Returns:
            Loaded configuration.

        Raises:
            FileNotFoundError: If no config file found.
            ValueError: If config file is invalid.
        """
        if path is None:
            path = cls._find_config()

        if not path.exists():
            raise FileNotFoundError(f"Config file not found: {path}")

        with open(path, "rb") as f:
            data = tomllib.load(f)

        return cls._from_dict(data, path)

    @classmethod
    def load_or_default(cls, path: Path | None = None) -> Config:
        """Load configuration or return default if not found."""
        try:
            return cls.load(path)
        except FileNotFoundError:
            return cls()

    @classmethod
    def _find_config(cls) -> Path:
        """Find config file by searching current directory and parents."""
        cwd = Path.cwd()
        for parent in [cwd, *cwd.parents]:
            config_path = parent / ".tambour" / "config.toml"
            if config_path.exists():
                return config_path

        # Return expected path even if it doesn't exist
        return cwd / ".tambour" / "config.toml"

    @classmethod
    def _from_dict(cls, data: dict[str, Any], path: Path) -> Config:
        """Create a Config from a dictionary."""
        tambour_section = data.get("tambour", {})
        version = tambour_section.get("version", "1")

        # Parse daemon config
        daemon_data = data.get("daemon", {})
        daemon = DaemonConfig(
            health_interval=daemon_data.get("health_interval", 60),
            zombie_threshold=daemon_data.get("zombie_threshold", 300),
            auto_recover=daemon_data.get("auto_recover", False),
        )

        # Parse agent config
        agent_data = data.get("agent", {})
        agent = AgentConfig(
            default_cli=agent_data.get("default_cli", "claude"),
        )

        # Parse worktree config
        worktree_data = data.get("worktree", {})
        worktree = WorktreeConfig(
            base_path=worktree_data.get("base_path", "../{repo}-worktrees"),
        )

        # Parse plugins
        plugins: dict[str, PluginConfig] = {}
        plugins_data = data.get("plugins", {})
        for name, plugin_data in plugins_data.items():
            plugins[name] = PluginConfig.from_dict(name, plugin_data)

        # Parse context providers
        context_providers: dict[str, ContextProviderConfig] = {}
        context_data = data.get("context", {})
        providers_data = context_data.get("providers", {})
        for name, provider_data in providers_data.items():
            context_providers[name] = ContextProviderConfig.from_dict(name, provider_data)

        return cls(
            version=version,
            daemon=daemon,
            agent=agent,
            worktree=worktree,
            plugins=plugins,
            context_providers=context_providers,
            config_path=path,
        )

    def get_plugins_for_event(self, event_type: str) -> list[PluginConfig]:
        """Get all enabled plugins that trigger on the given event type."""
        return [
            plugin
            for plugin in self.plugins.values()
            if plugin.matches_event(event_type) and plugin.enabled
        ]

    def get_enabled_context_providers(self) -> list[ContextProviderConfig]:
        """Get all enabled context providers sorted by order (lowest first)."""
        providers = [p for p in self.context_providers.values() if p.enabled]
        return sorted(providers, key=lambda p: p.order)

    def get_value(self, key_path: str) -> Any:
        """Get a configuration value by dot-separated path.

        Args:
            key_path: Dot-separated path to value (e.g. "agent.default_cli").

        Returns:
            The configuration value.

        Raises:
            KeyError: If path is invalid.
        """
        parts = key_path.split(".")
        current = self
        for part in parts:
            if hasattr(current, part):
                current = getattr(current, part)
            elif isinstance(current, dict) and part in current:
                current = current[part]
            else:
                raise KeyError(f"Invalid config path: {key_path}")
        return current
