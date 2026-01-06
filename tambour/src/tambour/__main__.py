"""CLI entry point for tambour.

Usage:
    python -m tambour <command> [options]

Commands:
    abort <issue> [--worktree-base PATH]
    agent [--cli CLI] [--issue ID] [--label LABEL]
    finish <issue> [--merge] [--no-continue]
    lock-status
    lock-release
    context collect [--prompt FILE] [--issue ID] [--worktree PATH] [--verbose]
    events emit <event> [--issue ID] [--worktree PATH]
    metrics collect [--storage PATH]
    metrics show [--window DAYS]
    metrics hot-files [--threshold N] [--window DAYS] [--limit N]
    metrics file <path>
    metrics session <session-id>
    metrics complexity [--threshold N]
    metrics clear [--older-than DAYS] [--dry-run]
    metrics refresh [--window DAYS]
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

    # agent command
    agent_parser = subparsers.add_parser(
        "agent", help="Spawn an AI agent on a beads issue"
    )
    agent_parser.add_argument(
        "--cli",
        help="Agent CLI to use (claude/gemini). Defaults to config value.",
    )
    agent_parser.add_argument(
        "--issue",
        help="Specific issue ID to work on. If not specified, picks next ready task.",
    )
    agent_parser.add_argument(
        "--label",
        help="Filter ready tasks by label (only used when --issue not specified).",
    )

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

    # abort command
    abort_parser = subparsers.add_parser(
        "abort", help="Abort/cancel agent work (unclaim issue, remove worktree, delete branch)"
    )
    abort_parser.add_argument("issue", help="Issue ID to abort")
    abort_parser.add_argument(
        "--worktree-base",
        help="Base directory for worktrees (default: ../bobbin-worktrees relative to git root)",
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

    # metrics show
    metrics_show_parser = metrics_subparsers.add_parser(
        "show", help="Display metrics summary"
    )
    metrics_show_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_show_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics hot-files
    metrics_hot_files_parser = metrics_subparsers.add_parser(
        "hot-files", help="List files by read count"
    )
    metrics_hot_files_parser.add_argument(
        "--threshold",
        "-t",
        type=int,
        default=5,
        help="Minimum read count to include (default: 5)",
    )
    metrics_hot_files_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_hot_files_parser.add_argument(
        "--limit",
        "-n",
        type=int,
        default=20,
        help="Maximum number of files to show (default: 20, 0 for unlimited)",
    )
    metrics_hot_files_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics file
    metrics_file_parser = metrics_subparsers.add_parser(
        "file", help="Show details for a specific file"
    )
    metrics_file_parser.add_argument(
        "path",
        help="File path to show metrics for",
    )
    metrics_file_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_file_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics session
    metrics_session_parser = metrics_subparsers.add_parser(
        "session", help="Show details for a specific session"
    )
    metrics_session_parser.add_argument(
        "session_id",
        help="Session ID to show metrics for",
    )
    metrics_session_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_session_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics complexity
    metrics_complexity_parser = metrics_subparsers.add_parser(
        "complexity", help="Show files with complexity signals"
    )
    metrics_complexity_parser.add_argument(
        "--threshold",
        "-t",
        type=float,
        default=3.0,
        help="Re-read rate threshold (default: 3.0)",
    )
    metrics_complexity_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_complexity_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics clear
    metrics_clear_parser = metrics_subparsers.add_parser(
        "clear", help="Remove old events from metrics"
    )
    metrics_clear_parser.add_argument(
        "--older-than",
        type=int,
        default=30,
        help="Remove events older than N days (default: 30)",
    )
    metrics_clear_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be removed without actually removing",
    )
    metrics_clear_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # metrics refresh
    metrics_refresh_parser = metrics_subparsers.add_parser(
        "refresh", help="Force refresh of cached aggregations"
    )
    metrics_refresh_parser.add_argument(
        "--window",
        type=int,
        default=7,
        help="Time window in days (default: 7)",
    )
    metrics_refresh_parser.add_argument(
        "--storage",
        help="Path to metrics.jsonl file",
    )

    # finish command
    finish_parser = subparsers.add_parser(
        "finish", help="Merge agent work and complete an issue"
    )
    finish_parser.add_argument("issue", help="Issue ID to finish")
    finish_parser.add_argument(
        "--merge",
        action="store_true",
        default=True,
        help="Merge the branch into main (default: True)",
    )
    finish_parser.add_argument(
        "--no-merge",
        action="store_false",
        dest="merge",
        help="Skip merging (keep worktree for later)",
    )
    finish_parser.add_argument(
        "--no-continue",
        action="store_true",
        help="Skip the 'continue to next task' flow after completion",
    )

    # lock-status command
    subparsers.add_parser("lock-status", help="Check merge lock status")

    # lock-release command
    subparsers.add_parser("lock-release", help="Force-release stuck merge lock")

    return parser


def cmd_agent(args: argparse.Namespace) -> int:
    """Handle 'agent' command."""
    from tambour.agent import AgentSpawner
    from tambour.config import Config

    config = Config.load_or_default()
    spawner = AgentSpawner(config)

    return spawner.spawn(
        cli=args.cli,
        issue_id=args.issue,
        label=args.label,
    )


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


def cmd_abort(args: argparse.Namespace) -> int:
    """Handle 'abort' command.

    Aborts/cancels agent work for an issue:
    1. Unclaim the issue (status=open, assignee="")
    2. Remove the worktree
    3. Delete the feature branch
    """
    import subprocess

    issue_id = args.issue
    print(f"Aborting {issue_id}...")

    # Determine worktree base path
    if args.worktree_base:
        worktree_base = Path(args.worktree_base)
    else:
        # Default: find git root and use ../bobbin-worktrees relative to it
        try:
            result = subprocess.run(
                ["git", "rev-parse", "--show-toplevel"],
                capture_output=True,
                text=True,
                check=True,
            )
            git_root = Path(result.stdout.strip())
            worktree_base = git_root.parent / "bobbin-worktrees"
        except subprocess.CalledProcessError:
            print("Error: Not in a git repository", file=sys.stderr)
            return 1

    worktree_path = worktree_base / issue_id

    # Step 1: Unclaim the issue (set status=open, clear assignee)
    print(f"  Unclaiming issue...")
    result = subprocess.run(
        ["bd", "update", issue_id, "--status", "open", "--assignee", ""],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        # Not a fatal error - issue may not exist or already be in open state
        if result.stderr:
            print(f"  Warning: {result.stderr.strip()}")

    # Step 2: Remove the worktree
    if worktree_path.exists():
        print(f"  Removing worktree at {worktree_path}...")
        # Use bd worktree remove with --force to handle uncommitted changes
        result = subprocess.run(
            ["bd", "worktree", "remove", str(worktree_path), "--force"],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            # Fall back to git worktree remove
            result = subprocess.run(
                ["git", "worktree", "remove", str(worktree_path), "--force"],
                capture_output=True,
                text=True,
            )
            if result.returncode != 0:
                # Last resort: just remove the directory
                import shutil

                try:
                    shutil.rmtree(worktree_path)
                    # Prune the worktree entry
                    subprocess.run(
                        ["git", "worktree", "prune"],
                        capture_output=True,
                        text=True,
                    )
                except Exception as e:
                    print(f"  Warning: Could not remove worktree: {e}", file=sys.stderr)
    else:
        print(f"  Worktree not found (already removed)")

    # Step 3: Delete the feature branch
    print(f"  Deleting branch {issue_id}...")
    result = subprocess.run(
        ["git", "branch", "-D", issue_id],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        # Not fatal - branch may not exist
        if "not found" not in result.stderr.lower() and result.stderr.strip():
            print(f"  Warning: {result.stderr.strip()}")

    print("Done.")
    return 0


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

    if args.command == "agent":
        sys.exit(cmd_agent(args))
    elif args.command == "context":
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
    elif args.command == "abort":
        sys.exit(cmd_abort(args))
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
        elif args.metrics_command == "show":
            from tambour.metrics.cli import cmd_metrics_show

            sys.exit(cmd_metrics_show(args))
        elif args.metrics_command == "hot-files":
            from tambour.metrics.cli import cmd_metrics_hot_files

            sys.exit(cmd_metrics_hot_files(args))
        elif args.metrics_command == "file":
            from tambour.metrics.cli import cmd_metrics_file

            sys.exit(cmd_metrics_file(args))
        elif args.metrics_command == "session":
            from tambour.metrics.cli import cmd_metrics_session

            sys.exit(cmd_metrics_session(args))
        elif args.metrics_command == "complexity":
            from tambour.metrics.cli import cmd_metrics_complexity

            sys.exit(cmd_metrics_complexity(args))
        elif args.metrics_command == "clear":
            from tambour.metrics.cli import cmd_metrics_clear

            sys.exit(cmd_metrics_clear(args))
        elif args.metrics_command == "refresh":
            from tambour.metrics.cli import cmd_metrics_refresh

            sys.exit(cmd_metrics_refresh(args))
        else:
            parser.parse_args(["metrics", "--help"])
            sys.exit(1)
    elif args.command == "finish":
        from tambour.finish import cmd_finish

        sys.exit(cmd_finish(args))
    elif args.command == "lock-status":
        from tambour.finish import cmd_lock_status

        sys.exit(cmd_lock_status(args))
    elif args.command == "lock-release":
        from tambour.finish import cmd_lock_release

        sys.exit(cmd_lock_release(args))
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
