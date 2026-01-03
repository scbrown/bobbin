"""Tambour - Context injection middleware for AI coding agents.

Tambour bridges Beads (task tracking), Bobbin (code indexing), and AI agents
to enable reliable parallel development with automatic context management.
"""

__version__ = "0.1.0"
__author__ = "Bobbin Team"

from tambour.events import EventType, Event, EventDispatcher
from tambour.config import Config, PluginConfig, ContextProviderConfig, VALID_EVENT_NAMES

__all__ = [
    "EventType",
    "Event",
    "EventDispatcher",
    "Config",
    "PluginConfig",
    "ContextProviderConfig",
    "VALID_EVENT_NAMES",
]
