"""CLI entry point for tambour.

Usage:
    python -m tambour <command> [options]

Commands:
    context collect [--prompt FILE] [--issue ID] [--worktree PATH] [--verbose]
    events emit <event> [--issue ID] [--worktree PATH]
    daemon start|stop|status
    config validate
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import NoReturn

from tambour import __version__


def create_parser() -> argparse.ArgumentParser:
    """Create the argument parser for the CLI."""
    parser = argparse.ArgumentParser(
        prog="tambour",
        description="Context injection middleware for AI coding agents",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # events command
    events_parser = subparsers.add_parser("events", help="Event management")
    events_subparsers = events_parser.add_subparsers(
        dest="events_command", help="Event subcommands"
    )

    # events emit
    emit_parser = events_subparsers.add_parser("emit", help="Emit an event")
    emit_parser.add_argument("event", help="Event type to emit")
    emit_parser.add_argument("--issue", help="Issue ID")
    emit_parser.add_argument("--worktree", help="Worktree path")

    # daemon command
    daemon_parser = subparsers.add_parser("daemon", help="Daemon management")
    daemon_parser.add_argument(
        "daemon_command",
        choices=["start", "stop", "status"],
        help="Daemon operation",
    )

    # config command
    config_parser = subparsers.add_parser("config", help="Configuration management")
    config_parser.add_argument(
        "config_command",
        choices=["validate"],
        help="Configuration operation",
    )

    # context command
    context_parser = subparsers.add_parser("context", help="Context provider management")
    context_subparsers = context_parser.add_subparsers(
        dest="context_command", help="Context subcommands"
    )

    # context collect
    collect_parser = context_subparsers.add_parser(
        "collect", help="Collect context from all providers"
    )
    collect_parser.add_argument(
        "--prompt",
        help="File containing the base prompt (use - for stdin)",
    )
    collect_parser.add_argument("--issue", help="Issue ID")
    collect_parser.add_argument("--worktree", help="Worktree path")
    collect_parser.add_argument("--main-repo", help="Main repository path")
    collect_parser.add_argument(
        "--verbose", "-v", action="store_true", help="Show provider execution details"
    )

    return parser


def cmd_context_collect(args: argparse.Namespace) -> int:
    """Handle 'context collect' command."""
    from tambour.config import Config
    from tambour.context import ContextCollector, ContextRequest

    # Read prompt from file or stdin
    prompt = ""
    if args.prompt:
        if args.prompt == "-":
            prompt = sys.stdin.read()
        else:
            prompt_path = Path(args.prompt)
            if prompt_path.exists():
                prompt = prompt_path.read_text()
            else:
                print(f"Error: Prompt file not found: {args.prompt}", file=sys.stderr)
                return 1

    # Build context request
    request = ContextRequest(
        prompt=prompt,
        issue_id=args.issue,
        worktree=Path(args.worktree) if args.worktree else None,
        main_repo=Path(args.main_repo) if args.main_repo else None,
    )

    config = Config.load_or_default()
    collector = ContextCollector(config)
    context, results = collector.collect(request)

    if args.verbose:
        providers = config.get_enabled_context_providers()
        if not providers:
            print("No context providers configured", file=sys.stderr)
        else:
            print(f"Ran {len(results)} context provider(s):", file=sys.stderr)
            for result in results:
                status = "OK" if result.success else "FAILED"
                duration = f" ({result.duration_ms}ms)" if result.duration_ms else ""
                print(f"  [{status}] {result.provider_name}{duration}", file=sys.stderr)
                if not result.success and result.error:
                    print(f"           {result.error}", file=sys.stderr)
            print("", file=sys.stderr)

    # Output the collected context
    if context:
        print(context)

    return 0


def cmd_events_emit(args: argparse.Namespace) -> int:
    """Handle 'events emit' command."""
    from tambour.events import EventType, Event, EventDispatcher
    from tambour.config import Config

    try:
        event_type = EventType(args.event)
    except ValueError:
        valid_events = ", ".join(e.value for e in EventType)
        print(f"Error: Unknown event type '{args.event}'", file=sys.stderr)
        print(f"Valid events: {valid_events}", file=sys.stderr)
        return 1

    event = Event(
        event_type=event_type,
        issue_id=args.issue,
        worktree=Path(args.worktree) if args.worktree else None,
    )

    config = Config.load_or_default()
    dispatcher = EventDispatcher(config)
    results = dispatcher.dispatch(event)

    if not results:
        print(f"Event '{event_type.value}' emitted (no plugins configured)")
        return 0

    failures = [r for r in results if not r.success]
    for result in results:
        status = "OK" if result.success else "FAILED"
        print(f"  [{status}] {result.plugin_name}")
        if not result.success and result.error:
            print(f"           {result.error}")

    return 1 if failures else 0


def cmd_daemon(args: argparse.Namespace) -> int:
    """Handle 'daemon' command."""
    from tambour.daemon import Daemon

    daemon = Daemon()

    if args.daemon_command == "start":
        return daemon.start()
    elif args.daemon_command == "stop":
        return daemon.stop()
    elif args.daemon_command == "status":
        return daemon.status()

    return 1


def cmd_config_validate(args: argparse.Namespace) -> int:
    """Handle 'config validate' command."""
    from tambour.config import Config

    try:
        config = Config.load()
        print(f"Configuration valid: {config.config_path}")
        print(f"  Version: {config.version}")
        print(f"  Plugins: {len(config.plugins)}")
        for name, plugin in config.plugins.items():
            status = "enabled" if plugin.enabled else "disabled"
            print(f"    - {name}: on={plugin.on}, {status}")
        print(f"  Context Providers: {len(config.context_providers)}")
        for name, provider in config.context_providers.items():
            status = "enabled" if provider.enabled else "disabled"
            print(f"    - {name}: order={provider.order}, {status}")
        return 0
    except FileNotFoundError as e:
        print(f"No configuration found: {e}", file=sys.stderr)
        return 0  # Missing config is not an error
    except Exception as e:
        print(f"Configuration error: {e}", file=sys.stderr)
        return 1


def main() -> NoReturn:
    """Main entry point."""
    parser = create_parser()
    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    if args.command == "context":
        if args.context_command == "collect":
            sys.exit(cmd_context_collect(args))
        else:
            parser.parse_args(["context", "--help"])
            sys.exit(1)
    elif args.command == "events":
        if args.events_command == "emit":
            sys.exit(cmd_events_emit(args))
        else:
            parser.parse_args(["events", "--help"])
            sys.exit(1)
    elif args.command == "daemon":
        sys.exit(cmd_daemon(args))
    elif args.command == "config":
        if args.config_command == "validate":
            sys.exit(cmd_config_validate(args))
        else:
            parser.parse_args(["config", "--help"])
            sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
