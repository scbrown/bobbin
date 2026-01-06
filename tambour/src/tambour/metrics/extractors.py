"""Tool-specific field extraction for metrics collection.

Each tool has different fields that are relevant for metrics. This module
provides extractors that normalize tool input/output to a standard format.
"""

from __future__ import annotations

from typing import Any


def extract_read_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Read tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with file_path, offset, and limit.
    """
    return {
        "file_path": tool_input.get("file_path"),
        "offset": tool_input.get("offset"),
        "limit": tool_input.get("limit"),
    }


def extract_write_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Write tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with file_path and content_length.
    """
    content = tool_input.get("content", "")
    return {
        "file_path": tool_input.get("file_path"),
        "content_length": len(content) if isinstance(content, str) else None,
    }


def extract_edit_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Edit tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with file_path, old_string_len, and new_string_len.
    """
    old_string = tool_input.get("old_string", "")
    new_string = tool_input.get("new_string", "")
    return {
        "file_path": tool_input.get("file_path"),
        "old_string_len": len(old_string) if isinstance(old_string, str) else None,
        "new_string_len": len(new_string) if isinstance(new_string, str) else None,
    }


def extract_glob_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Glob tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with pattern and path.
    """
    return {
        "pattern": tool_input.get("pattern"),
        "path": tool_input.get("path"),
    }


def extract_grep_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Grep tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with pattern, path, and output_mode.
    """
    return {
        "pattern": tool_input.get("pattern"),
        "path": tool_input.get("path"),
        "output_mode": tool_input.get("output_mode"),
    }


def extract_bash_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Bash tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with command_prefix (first token) and description.
    """
    command = tool_input.get("command", "")
    # Extract first token as command prefix
    command_prefix = None
    if isinstance(command, str) and command.strip():
        command_prefix = command.strip().split()[0]

    return {
        "command_prefix": command_prefix,
        "description": tool_input.get("description"),
    }


def extract_webfetch_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from WebFetch tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with url and prompt.
    """
    return {
        "url": tool_input.get("url"),
        "prompt": tool_input.get("prompt"),
    }


def extract_websearch_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from WebSearch tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with query.
    """
    return {
        "query": tool_input.get("query"),
    }


def extract_task_fields(tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract fields from Task tool input.

    Args:
        tool_input: The tool_input dict from the event.

    Returns:
        Dict with subagent_type and description.
    """
    return {
        "subagent_type": tool_input.get("subagent_type"),
        "description": tool_input.get("description"),
    }


# Mapping from tool name to extractor function
TOOL_EXTRACTORS: dict[str, callable] = {
    "Read": extract_read_fields,
    "Write": extract_write_fields,
    "Edit": extract_edit_fields,
    "Glob": extract_glob_fields,
    "Grep": extract_grep_fields,
    "Bash": extract_bash_fields,
    "WebFetch": extract_webfetch_fields,
    "WebSearch": extract_websearch_fields,
    "Task": extract_task_fields,
}


def extract_tool_fields(tool_name: str, tool_input: dict[str, Any]) -> dict[str, Any]:
    """Extract relevant fields from tool input based on tool type.

    Args:
        tool_name: Name of the tool (Read, Write, Edit, etc.).
        tool_input: The raw tool_input dict from the event.

    Returns:
        Dict with tool-specific fields extracted.
        Returns the original tool_input for unknown tools.
    """
    extractor = TOOL_EXTRACTORS.get(tool_name)
    if extractor:
        return extractor(tool_input)
    # For unknown tools, return the input as-is (with limits)
    return _limit_dict_size(tool_input)


def _limit_dict_size(d: dict[str, Any], max_str_len: int = 200) -> dict[str, Any]:
    """Limit string values in a dict to prevent huge metrics entries.

    Args:
        d: The dict to limit.
        max_str_len: Maximum length for string values.

    Returns:
        Dict with string values truncated.
    """
    result = {}
    for key, value in d.items():
        if isinstance(value, str) and len(value) > max_str_len:
            result[key] = value[:max_str_len] + "..."
        elif isinstance(value, dict):
            result[key] = _limit_dict_size(value, max_str_len)
        else:
            result[key] = value
    return result
