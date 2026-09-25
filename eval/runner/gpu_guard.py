#!/usr/bin/env python3
"""Pinned Bobbin proxy: yield to a hold command and refuse CPU fallback.

Copied beside bobbin.real and gpu-runtime.json by the pilot. The hold command
returns 0 clear, 1 held, 2 unknown; every nonzero/failed probe pauses GPU work.
"""
from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

if __package__:
    from runner.runtime_lock import verify_and_record
else:
    from runtime_lock import verify_and_record

PROOF = "ONNX session using CUDA GPU acceleration"


def held(config):
    try:
        result = subprocess.run(config["hold_command"],
                                env=dict(os.environ, **config.get("hold_env", {})),
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=10, check=False)
        return result.returncode != 0
    except (OSError, subprocess.TimeoutExpired):
        return True


def supervise(command, config, *, require_proof=False, interval=2, receipt_path=None):
    while held(config):
        print("GPU paused: hold active or unreadable", file=sys.stderr, flush=True)
        time.sleep(interval)
    # Check AFTER any hold, immediately before the child can index/search.
    if receipt_path is None:
        raise ValueError('runtime verification receipt path is required')
    verify_and_record(config, receipt_path)
    env = dict(os.environ, **config["environment"])
    env["BOBBIN_GPU"] = "1"
    proc = subprocess.Popen(command, env=env, stderr=subprocess.PIPE, start_new_session=True)
    proof = threading.Event()
    fallback = threading.Event()

    def read_errors():
        assert proc.stderr is not None
        for line in iter(proc.stderr.readline, b""):
            sys.stderr.buffer.write(line)
            sys.stderr.buffer.flush()
            if PROOF.encode() in line:
                proof.set()
            if b"falling back to CPU" in line or b"using CPU at" in line:
                fallback.set()

    reader = threading.Thread(target=read_errors, daemon=True)
    reader.start()
    paused = False

    def send(sig):
        try:
            os.killpg(proc.pid, sig)
        except ProcessLookupError:
            pass

    def terminate(_sig, _frame):
        send(signal.SIGCONT)
        send(signal.SIGTERM)
        raise SystemExit(128 + _sig)

    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, terminate)
    try:
        while proc.poll() is None:
            if fallback.is_set():
                send(signal.SIGCONT)
                send(signal.SIGTERM)
                proc.wait(timeout=10)
                print("GPU proof failed: CPU fallback refused", file=sys.stderr)
                return 2
            pause = held(config)
            if pause != paused:
                send(signal.SIGSTOP if pause else signal.SIGCONT)
                paused = pause
                print("GPU paused for hold" if pause else "GPU resumed after hold",
                      file=sys.stderr, flush=True)
            time.sleep(interval)
        reader.join(timeout=5)
        if fallback.is_set() or (require_proof and not proof.is_set()):
            print("GPU proof failed: explicit CUDA session receipt missing", file=sys.stderr)
            return 2
        return proc.returncode
    finally:
        if proc.poll() is None:
            send(signal.SIGCONT)
            send(signal.SIGKILL)
            proc.wait()


def main():
    root = Path(__file__).resolve().parent
    config = json.loads((root / "gpu-runtime.json").read_text())
    receipts = root / 'runtime-checks'
    receipts.mkdir(exist_ok=True)
    return supervise([str(root / "bobbin.real"), *sys.argv[1:]], config,
                     require_proof="index" in sys.argv[1:], interval=config.get("poll_seconds", 2),
                     receipt_path=receipts / f'{time.time_ns()}-{os.getpid()}.json')


if __name__ == "__main__":
    raise SystemExit(main())
