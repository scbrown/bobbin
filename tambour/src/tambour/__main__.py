"""CLI entry point for tambour.

Usage:
    python -m tambour <command> [options]

Commands:
    context collect [--prompt FILE] [--issue ID] [--worktree PATH] [--verbose]
    events emit <event> [--issue ID] [--worktree PATH]
    metrics collect [--storage PATH]
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
    emit_parser.add_argument("--main-repo", help="Main repository path")
    emit_parser.add_argument("--beads-db", help="Beads database path")
    emit_parser.add_argument(
        "--data",
        help="JSON payload with event data (alternative to --extra flags)",
    )
    emit_parser.add_argument(
        "--extra",
        action="append",
        help="Extra data (key=value). Can be used multiple times.",
    )

    # daemon command
    daemon_parser = subparsers.add_parser("daemon", help="Daemon management")
    daemon_parser.add_argument(
        "daemon_command",
        choices=["start", "stop", "status"],
        help="Daemon operation",
    )

    # config command
    config_parser = subparsers.add_parser("config", help="Configuration management")
    config_subparsers = config_parser.add_subparsers(
        dest="config_command", help="Config subcommands"
    )

    # config validate
    config_subparsers.add_parser("validate", help="Validate configuration")

    # config get
    get_parser = config_subparsers.add_parser("get", help="Get configuration value")
    get_parser.add_argument("key", help="Configuration key (e.g. agent.default_cli)")

    # heartbeat command
    heartbeat_parser = subparsers.add_parser("heartbeat", help="Start heartbeat writer")
    heartbeat_parser.add_argument("worktree", help="Worktree path")
    heartbeat_parser.add_argument(
        "--interval",
        type=int,
        default=30,
        help="Heartbeat interval in seconds",
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

    # metrics command
    metrics_parser = subparsers.add_parser("metrics", help="Metrics collection")
    metrics_subparsers = metrics_parser.add_subparsers(
        dest="metrics_command", help="Metrics subcommands"
    )

    # metrics collect
    metrics_collect_parser = metrics_subparsers.add_parser(
        "collect", help="Collect metrics from event (plugin entry point)"
    )
    metrics_collect_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file (default: .tambour/metrics.jsonl)",
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
    import json

    from tambour.config import Config
    from tambour.events import Event, EventDispatcher, EventType

    try:
        event_type = EventType(args.event)
    except ValueError:
        valid_events = ", ".join(e.value for e in EventType)
        print(f"Error: Unknown event type '{args.event}'", file=sys.stderr)
        print(f"Valid events: {valid_events}", file=sys.stderr)
        return 1

    # Build extra dict from --data JSON or --extra flags
    extra: dict[str, str] = {}

    if args.data:
        try:
            data = json.loads(args.data)
            if isinstance(data, dict):
                # Flatten nested dicts to string values for env var compatibility
                for key, value in data.items():
                    if isinstance(value, dict):
                        extra[key] = json.dumps(value)
                    else:
                        extra[key] = str(value)
        except json.JSONDecodeError as e:
            print(f"Error: Invalid JSON in --data: {e}", file=sys.stderr)
            return 1

    if args.extra:
        for item in args.extra:
            if "=" in item:
                key, value = item.split("=", 1)
                extra[key] = value

    event = Event(
        event_type=event_type,
        issue_id=args.issue,
        worktree=Path(args.worktree) if args.worktree else None,
        main_repo=Path(args.main_repo) if args.main_repo else None,
        beads_db=Path(args.beads_db) if args.beads_db else None,
        extra=extra,
    )

    config = Config.load_or_default()

    # Use a log file for async execution results
    log_file = Path.home() / ".tambour" / "events.log"
    log_file.parent.mkdir(parents=True, exist_ok=True)

    dispatcher = EventDispatcher(config, log_file=log_file)
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


def cmd_config_get(args: argparse.Namespace) -> int:
    """Handle 'config get' command."""
    from tambour.config import Config

    try:
        config = Config.load_or_default()
        value = config.get_value(args.key)
        print(value)
        return 0
    except KeyError:
        print(f"Error: Config key not found: {args.key}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"Error reading config: {e}", file=sys.stderr)
        return 1


def cmd_config_validate(args: argparse.Namespace) -> int:
    """Handle 'config validate' command."""
    from tambour.config import Config

    try:
        config = Config.load()
        print(f"Configuration valid: {config.config_path}")
        print(f"  Version: {config.version}")
        print(f"  Agent CLI: {config.agent.default_cli}")
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


def cmd_heartbeat(args: argparse.Namespace) -> int:
    """Handle 'heartbeat' command."""
    from tambour.heartbeat import HeartbeatWriter

    worktree = Path(args.worktree)
    writer = HeartbeatWriter(worktree, interval=args.interval)
    writer.start()
    return 0


def cmd_metrics_collect(args: argparse.Namespace) -> int:
    """Handle 'metrics collect' command.

    This is the plugin entry point for the metrics-collector plugin.
    It collects metric data from environment variables (set by the event
    dispatcher) and stores it to JSONL.
    """
    from tambour.metrics.collector import MetricsCollector

    storage_path = Path(args.storage) if args.storage else None
    collector = MetricsCollector(storage_path=storage_path)
    collector.collect_and_store()
    # Always return 0 to not block event dispatch
    return 0


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
        elif args.config_command == "get":
            sys.exit(cmd_config_get(args))
        else:
            parser.parse_args(["config", "--help"])
            sys.exit(1)
    elif args.command == "heartbeat":
        sys.exit(cmd_heartbeat(args))
    elif args.command == "metrics":
        if args.metrics_command == "collect":
            sys.exit(cmd_metrics_collect(args))
        else:
            parser.parse_args(["metrics", "--help"])
            sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
