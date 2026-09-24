"""GPU proof and gaming-hold discrimination without a GPU or real hold writes."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

from runner.gpu_guard import held

ROOT = Path(__file__).resolve().parents[1]


def fixture(tmp_path, body, status=0):
    state = tmp_path / 'hold'
    state.write_text(str(status))
    shutil.copyfile(ROOT / 'runner/gpu_guard.py', tmp_path / 'bobbin')
    real = tmp_path / 'bobbin.real'
    real.write_text('#!' + sys.executable + '\n' + body)
    real.chmod(0o755)
    config = {'environment': {}, 'poll_seconds': 0.05,
              'hold_command': [sys.executable, '-c',
                               'import pathlib,sys;sys.exit(int(pathlib.Path(sys.argv[1]).read_text()))', str(state)]}
    (tmp_path / 'gpu-runtime.json').write_text(json.dumps(config))
    return state, config


def test_requires_positive_receipt(tmp_path):
    fixture(tmp_path, 'print("normal completion")\n')
    p = subprocess.run([sys.executable, str(tmp_path / 'bobbin'), 'index'], capture_output=True)
    assert p.returncode == 2
    assert b'explicit CUDA session receipt missing' in p.stderr


def test_fallback_is_failure_even_with_marker(tmp_path):
    fixture(tmp_path, 'import sys\nprint("ONNX session using CUDA GPU acceleration",file=sys.stderr)\n'
                     'print("warning: falling back to CPU",file=sys.stderr)\n')
    p = subprocess.run([sys.executable, str(tmp_path / 'bobbin'), 'index'], capture_output=True)
    assert p.returncode == 2


def test_hold_and_unknown_pause_then_resume(tmp_path):
    ticks = tmp_path / 'ticks'
    state, config = fixture(tmp_path, 'import sys,time\n'
                           'print("ONNX session using CUDA GPU acceleration",file=sys.stderr,flush=True)\n'
                           f'with open({str(ticks)!r},"a",buffering=1) as f:\n'
                           ' for i in range(500):\n  f.write("x\\n");time.sleep(.02)\n')
    p = subprocess.Popen([sys.executable, str(tmp_path / 'bobbin'), 'index'],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 5
        while not ticks.exists() and time.monotonic() < deadline:
            time.sleep(.05)
        assert ticks.exists()
        for status in (1, 2):
            state.write_text(str(status))
            assert held(config)
            time.sleep(.3)
            size = ticks.stat().st_size
            time.sleep(.2)
            assert ticks.stat().st_size == size
            state.write_text('0')
            time.sleep(.3)
            assert ticks.stat().st_size > size
    finally:
        p.terminate()
        p.wait(timeout=5)


def test_missing_probe_fails_closed():
    assert held({'hold_command': ['/nonexistent-hold-command']})
