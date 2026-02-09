"""CLI entrypoint for the bobbin eval runner."""

import click


@click.group()
def cli():
    """Bobbin evaluation framework — compare Claude Code with and without bobbin."""
    pass


@cli.command()
@click.argument("task_id")
@click.option("--attempts", default=3, help="Number of attempts per approach.")
@click.option("--approaches", default="both", type=click.Choice(["no-bobbin", "with-bobbin", "both"]))
def run_task(task_id: str, attempts: int, approaches: str):
    """Run evaluation for a single task."""
    click.echo(f"Running task {task_id} ({attempts} attempts, approaches={approaches})")
    raise NotImplementedError("Runner not yet implemented — see bo-4iep")


@cli.command()
@click.option("--tasks-dir", default="tasks", help="Directory containing task YAML files.")
@click.option("--attempts", default=3)
def run_all(tasks_dir: str, attempts: int):
    """Run evaluation for all tasks in the tasks directory."""
    click.echo(f"Running all tasks from {tasks_dir} ({attempts} attempts each)")
    raise NotImplementedError("Runner not yet implemented — see bo-4iep")


if __name__ == "__main__":
    cli()
