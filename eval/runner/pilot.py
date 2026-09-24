"""Preregistered L1 pilot: independent tool and injection switches.

Run with ``python -m runner.pilot --output <new-directory>``. This command
only schedules the committed 30-task, one-repetition pilot, never the full study.
Failed fixture controls stop a task before spending any agent budget.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import tempfile
import time
from contextlib import contextmanager
from pathlib import Path

from runner.agent_runner import _find_claude, parse_stream_json
from runner.bobbin_setup import _find_bobbin, setup_bobbin
from runner.l0 import _TEST_FILE, _TEST_PATH, SEED, gold_for_commit
from runner.task_loader import load_task_by_id
from runner.workspace import checkout_parent, clone_repo
from scorer.attribution import serving_model_from_usage
from scorer.test_scorer import _parse_output

ROOT = Path(__file__).resolve().parent.parent
CELLS = ("none", "tool", "inject", "both")
MODEL = "claude-sonnet-5"
CUTOFF = "2026-01"
CUTOFF_SOURCE = "https://platform.claude.com/docs/en/models/sonnet-5/overview"


def schedule(task_ids):
    rng = random.Random(SEED)
    result = []
    for task in task_ids:
        cells = list(CELLS)
        rng.shuffle(cells)
        result.extend((task, cell) for cell in cells)
    return result


def cell_config(cell, bobbin):
    if cell not in CELLS:
        raise ValueError(cell)
    tool, inject = cell in ("tool", "both"), cell in ("inject", "both")
    settings = {"hooks": {}}
    if inject:
        settings["hooks"]["UserPromptSubmit"] = [{"hooks": [{
            "type": "command", "command": f"{shlex_quote(bobbin)} hook inject-context --budget 300",
            "timeout": 120,
        }]}]
    mcp = {"mcpServers": {}}
    if tool:
        mcp["mcpServers"]["bobbin"] = {"command": bobbin, "args": ["serve"],
                                        "env": {"BOBBIN_SERVER": ""}}
    # CLI search would make the tool-off arms tool-on. The hook is invoked by
    # the harness, not by Bash, so this does not switch off auto injection.
    denied = ["Bash(bobbin *)", f"Bash({bobbin} *)"]
    return settings, mcp, denied


def shlex_quote(value):
    import shlex
    return shlex.quote(value)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def run(cmd, cwd, env, timeout=900):
    return subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True,
                          timeout=timeout, check=True)


@contextmanager
def process_env(env):
    old = os.environ.copy()
    try:
        os.environ.clear()
        os.environ.update(env)
        yield
    finally:
        os.environ.clear()
        os.environ.update(old)


def isolated_env(home):
    """Copy auth, never symlink or mutate the real user's configuration."""
    real_home = Path.home()
    home.mkdir(mode=0o700)
    config = home / ".claude"
    config.mkdir(mode=0o700)
    credentials = real_home / ".claude" / ".credentials.json"
    if credentials.exists():
        shutil.copyfile(credentials, config / ".credentials.json")
        (config / ".credentials.json").chmod(0o600)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("BOBBIN_", "CLAUDE_", "SHANTY_", "GT_"))
           and k not in ("CLAUDECODE", "BASH_ENV")}
    env.update(HOME=str(home), CLAUDE_CONFIG_DIR=str(config),
               XDG_CONFIG_HOME=str(home / ".config"), BOBBIN_SERVER="",
               RUSTUP_HOME=os.environ.get("RUSTUP_HOME", str(real_home / ".rustup")),
               CARGO_HOME=os.environ.get("CARGO_HOME", str(real_home / ".cargo")),
               GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
    runtime = real_home / ".local/lib/onnxruntime/libonnxruntime.so"
    if "ORT_DYLIB_PATH" not in env and runtime.exists():
        env["ORT_DYLIB_PATH"] = str(runtime)
    return env


def grade(ws, command, env, timeout):
    """A successful exit without evidence of executed tests is not a pass."""
    start = time.monotonic()
    try:
        p = subprocess.run(["sh", "-c", command], cwd=ws, env=env,
                           capture_output=True, text=True, timeout=timeout, check=False)
        output, rc = p.stdout + p.stderr, p.returncode
    except subprocess.TimeoutExpired:
        return {"passed": False, "valid": False, "reason": "test_timeout"}
    counts = _parse_output(output)
    if not counts:
        m = re.search(r"Ran (\d+) tests? in", output)
        if m:
            counts = {"total": int(m[1]), "skipped": 0}
    executed = counts.get("total", 0) - counts.get("skipped", 0)
    return {"passed": rc == 0 and executed > 0, "valid": executed > 0,
            "executed": executed, "exit_code": rc, "output": output[-50000:],
            "duration_seconds": time.monotonic() - start}


def hidden_test_paths(ws, commit, env):
    paths = run(["git", "diff", "--name-only", "--diff-filter=AM", f"{commit}^", commit],
                ws, env).stdout.splitlines()
    return [p for p in paths if _TEST_PATH.search(p) or _TEST_FILE.search(p)]


def install_tests(ws, commit, paths, env):
    if paths:
        run(["git", "checkout", commit, "--", *paths], ws, env)


def cell_checkout(source, dest, env):
    """Keep the parent files and prepared index, but remove the answer's history."""
    tracked = run(["git", "ls-files", "-z"], source, env).stdout
    # Preparation deliberately removes agent guidance for every experimental
    # arm, including repositories that track it. Preserve every other path
    # literally, even when it is ignored or contains pathspec metacharacters.
    tracked = "".join(path + "\0" for path in tracked.split("\0")
                      if path and path != ".claude" and not path.startswith(".claude/"))
    shutil.copytree(source, dest, symlinks=True, ignore=shutil.ignore_patterns(".git"))
    run(["git", "init", "-q"], dest, env)
    run(["git", "config", "core.hooksPath", "/dev/null"], dest, env)
    subprocess.run(["git", "--literal-pathspecs", "add", "--force",
                    "--pathspec-from-file=-", "--pathspec-file-nul"],
                   input=tracked, cwd=dest, env=env, text=True, check=True, capture_output=True)
    run(["git", "-c", "user.name=Eval", "-c", "user.email=eval@example.invalid",
         "commit", "-qm", "Evaluation starting tree"], dest, env)
    return run(["git", "rev-parse", "HEAD"], dest, env).stdout.strip()


def grade_tests_from_source(source, ws, commit, paths, env):
    for path in paths:
        content = subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=source, env=env)
        target = ws / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)


def prepare(task, directory, env, timeout):
    ws = clone_repo(task["repo"], str(directory), cache_dir=directory.parent / "repos")
    parent = checkout_parent(ws, task["commit"])
    run(["git", "config", "core.hooksPath", "/dev/null"], ws, env)
    if task.get("setup_command"):
        run(["sh", "-c", task["setup_command"]], ws, env, timeout)
    test_paths = hidden_test_paths(ws, task["commit"], env)
    install_tests(ws, task["commit"], test_paths, env)
    negative = grade(ws, task["test_command"], env, timeout)
    # The positive control uses the exact fixing tree and the identical command.
    run(["git", "checkout", "--detach", "-f", task["commit"]], ws, env)
    positive = grade(ws, task["test_command"], env, timeout)
    controls = {"parent": negative, "fix": positive, "hidden_test_paths": test_paths}
    write_json(directory / "controls.json", controls)
    if not negative["valid"] or negative["passed"] or not positive["passed"]:
        raise ValueError("fixture does not discriminate parent from fix; see controls.json")
    run(["git", "checkout", "--detach", "-f", parent], ws, env)
    return ws, parent, test_paths


def invoke(task, cell, ws, out, env, bobbin, claude, budget, timeout, turns):
    settings, mcp, denied = cell_config(cell, bobbin)
    write_json(out / "settings.json", settings)
    write_json(out / "mcp.json", mcp)
    # Identical text in all four cells; tool availability is advertised by MCP.
    prompt = (f"You are working on {task['repo']}.\n\n{task['description'].strip()}\n\n"
              "Implement the fix in this checkout. Do not inspect git history or use network "
              "services to find the fix. The evaluator supplies hidden tests after your work.")
    cmd = [claude, "-p", prompt, "--model", MODEL, "--output-format", "stream-json",
           "--verbose", "--max-budget-usd", str(budget), "--max-turns", str(turns),
           "--permission-mode", "bypassPermissions", "--setting-sources", "",
           "--tools", "Bash,Read,Write,Edit,Glob,Grep",
           "--settings", str(out / "settings.json"), "--strict-mcp-config",
           "--mcp-config", str(out / "mcp.json"), "--disallowedTools", *denied]
    start = time.monotonic()
    with (out / "stream.jsonl").open("w") as stdout, (out / "stderr.txt").open("w") as stderr:
        try:
            p = subprocess.run(cmd, cwd=ws, env=env, stdout=stdout, stderr=stderr,
                               timeout=timeout, check=False)
            rc = p.returncode
        except subprocess.TimeoutExpired:
            rc = -1
    parsed = parse_stream_json((out / "stream.jsonl").read_text())
    result = parsed["result_line"] or {}
    model = serving_model_from_usage(result.get("modelUsage"))
    return {"exit_code": rc, "duration_seconds": time.monotonic() - start,
            "serving_model": model, "result": result,
            "tool_use_summary": parsed["tool_use_summary"],
            "valid": rc == 0 and model == MODEL and not result.get("is_error", True)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--budget", type=float, default=2.0)
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--max-turns", type=int, default=40)
    parser.add_argument("--gpu-runtime", type=Path, help="Pinned runtime/hold configuration")
    parser.add_argument("--bobbin", type=Path, help="Explicit executable to pin")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    tasks = (ROOT / "v2/pilot-tasks.txt").read_text().splitlines()
    tasks = [t.strip() for t in tasks if t.strip() and not t.startswith("#")]
    assert len(tasks) == 30 and len(set(tasks)) == 30
    order = schedule(tasks)
    bobbin, claude = str(args.bobbin) if args.bobbin else _find_bobbin(), _find_claude()
    # Freeze the binary: a host CLI updater must not change the treatment mid-study.
    pinned = out / "bin"
    pinned.mkdir()
    shutil.copy2(bobbin, pinned / "bobbin")
    binary_hash = hashlib.sha256((pinned / "bobbin").read_bytes()).hexdigest()
    runtime = None
    if args.gpu_runtime:
        runtime = json.loads(args.gpu_runtime.read_text())
        (pinned / "bobbin").rename(pinned / "bobbin.real")
        shutil.copy2(ROOT / "runner/gpu_guard.py", pinned / "bobbin")
        (pinned / "bobbin").chmod(0o755)
        write_json(pinned / "gpu-runtime.json", runtime)
    bobbin = str(pinned / "bobbin")
    write_json(out / "manifest.json", {
        "kind": "pilot", "schedule": order, "model": MODEL,
        "model_training_cutoff": CUTOFF, "cutoff_source": CUTOFF_SOURCE,
        "budget_per_run": args.budget, "timeout": args.timeout, "max_turns": args.max_turns,
        "preregistration_sha256": hashlib.sha256((ROOT / "v2/PREREGISTRATION.md").read_bytes()).hexdigest(),
        "harness_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "bobbin_sha256": binary_hash,
        "embedding_runtime": runtime,
        "gpu_guard_sha256": hashlib.sha256(Path(bobbin).read_bytes()).hexdigest() if runtime else None,
    })
    for task_id in tasks:
        task = load_task_by_id(task_id, ROOT / "tasks")
        task_out = out / task_id
        task_out.mkdir()
        with tempfile.TemporaryDirectory(prefix="pilot-", dir=out) as temp:
            scratch = Path(temp)
            env = isolated_env(scratch / "home")
            env["PATH"] = str(pinned) + os.pathsep + env["PATH"]
            try:
                ws, _parent, tests = prepare(task, scratch, env, args.timeout)
                shutil.copy2(scratch / "controls.json", task_out / "controls.json")
                # One index at the pre-fix tree; all cells receive identical copies.
                with process_env(env):
                    index_metadata = setup_bobbin(str(ws), timeout=args.timeout)
                write_json(task_out / "index.json", index_metadata)
                if runtime and not index_metadata.get("gpu_acceleration_proven"):
                    raise RuntimeError("GPU index receipt missing; pilot stopped before spend")
                shutil.rmtree(ws / ".claude")  # setup's v1 arm-specific instructions
                gold = gold_for_commit(ws, task["commit"])
                for _, cell in (pair for pair in order if pair[0] == task_id):
                    cell_out = task_out / cell
                    cell_out.mkdir()
                    # Bobbin's local repo identity can derive from the directory
                    # name. Keep it identical to the indexed source in every cell.
                    cell_root = scratch / cell
                    cell_root.mkdir()
                    cell_ws = cell_root / ws.name
                    baseline = cell_checkout(ws, cell_ws, env)
                    agent = invoke(task, cell, cell_ws, cell_out, env, bobbin, claude,
                                   args.budget, args.timeout, args.max_turns)
                    diff = run(["git", "diff", baseline], cell_ws, env).stdout
                    (cell_out / "agent.diff").write_text(diff)
                    touched = set(run(["git", "diff", "--name-only", baseline], cell_ws, env).stdout.splitlines())
                    grade_tests_from_source(ws, cell_ws, task["commit"], tests, env)
                    test_result = grade(cell_ws, task["test_command"], env, args.timeout)
                    precision = len(touched & gold.keys()) / len(touched) if touched else 0
                    recall = len(touched & gold.keys()) / len(gold) if gold else 0
                    result = {"task": task_id, "cell": cell, "agent": agent, "tests": test_result,
                              "success": agent["valid"] and test_result["passed"],
                              "file_f1": 2 * precision * recall / (precision + recall) if precision + recall else 0}
                    write_json(cell_out / "result.json", result)
                    print(f"{task_id} {cell}: success={result['success']}", flush=True)
                    shutil.rmtree(cell_root)
                    if not agent["valid"]:
                        raise RuntimeError("agent unavailable/misattributed; pilot stopped before further spend")
            except ValueError as exc:
                if (scratch / "controls.json").exists():
                    shutil.copy2(scratch / "controls.json", task_out / "controls.json")
                write_json(task_out / "fixture-error.json", {"error": str(exc)})
                print(f"{task_id}: fixture_error: {exc}", flush=True)


if __name__ == "__main__":
    main()
