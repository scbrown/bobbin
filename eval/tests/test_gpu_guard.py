"""GPU proof and gaming-hold discrimination without a GPU or real hold writes."""
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

from runner.gpu_guard import held

ROOT = Path(__file__).resolve().parents[1]


def fixture(tmp_path, body, runtime_config, status=0):
    state = tmp_path / 'hold'
    state.write_text(str(status))
    shutil.copyfile(ROOT / 'runner/gpu_guard.py', tmp_path / 'bobbin')
    shutil.copyfile(ROOT / 'runner/runtime_lock.py', tmp_path / 'runtime_lock.py')
    real = tmp_path / 'bobbin.real'
    real.write_text('#!' + sys.executable + '\n' + body)
    real.chmod(0o755)
    config = {**runtime_config, 'poll_seconds': 0.05,
              'hold_command': [sys.executable, '-c',
                               'import pathlib,sys;sys.exit(int(pathlib.Path(sys.argv[1]).read_text()))', str(state)]}
    (tmp_path / 'gpu-runtime.json').write_text(json.dumps(config))
    return state, config


def test_requires_positive_receipt(tmp_path, runtime_config):
    fixture(tmp_path, 'print("normal completion")\n', runtime_config)
    p = subprocess.run([sys.executable, str(tmp_path / 'bobbin'), 'index'], capture_output=True, check=False)
    assert p.returncode == 2
    assert b'explicit CUDA session receipt missing' in p.stderr


def test_fallback_is_failure_even_with_marker(tmp_path, runtime_config):
    fixture(tmp_path, 'import sys\nprint("ONNX session using CUDA GPU acceleration",file=sys.stderr)\n'
                     'print("warning: falling back to CPU",file=sys.stderr)\n', runtime_config)
    p = subprocess.run([sys.executable, str(tmp_path / 'bobbin'), 'index'], capture_output=True, check=False)
    assert p.returncode == 2


def test_hold_and_unknown_pause_then_resume(tmp_path, runtime_config):
    ticks = tmp_path / 'ticks'
    state, config = fixture(tmp_path, 'import sys,time\n'
                           'print("ONNX session using CUDA GPU acceleration",file=sys.stderr,flush=True)\n'
                           f'with open({str(ticks)!r},"a",buffering=1) as f:\n'
                           ' for i in range(500):\n  f.write("x\\n");time.sleep(.02)\n', runtime_config)
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


def test_tampered_library_never_launches_wrapped_command(tmp_path, runtime_config):
    marker = tmp_path / 'indexed'
    fixture(tmp_path, f'open({str(marker)!r},"w").write("started")\n', runtime_config)
    Path(next(iter(runtime_config['libraries']))).write_bytes(b'tampered')
    p = subprocess.run([sys.executable, str(tmp_path / 'bobbin'), 'index'], capture_output=True, check=False)
    assert p.returncode != 0
    assert b'library SHA256 mismatch' in p.stderr
    assert not marker.exists()
    receipts = list((tmp_path / 'runtime-checks').glob('*.json'))
    assert len(receipts) == 1
    assert json.loads(receipts[0].read_text())['status'] == 'refused'


def test_hash_is_checked_after_hold_clears(tmp_path, runtime_config):
    marker = tmp_path / 'indexed'
    state, _ = fixture(tmp_path, f'open({str(marker)!r},"w").write("started")\n',
                       runtime_config, status=1)
    p = subprocess.Popen([sys.executable, str(tmp_path / 'bobbin'), 'index'],
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        assert b'GPU paused' in p.stderr.readline()
        Path(next(iter(runtime_config['libraries']))).write_bytes(b'tampered while held')
        state.write_text('0')
        _, stderr = p.communicate(timeout=5)
        assert p.returncode != 0
        assert b'library SHA256 mismatch' in stderr
        assert not marker.exists()
    finally:
        if p.poll() is None:
            p.kill()
            p.wait()
