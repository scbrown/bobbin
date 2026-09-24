"""Discriminating hash/loader and campaign-start controls without real CUDA."""
import json
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

from runner import pilot
from runner.runtime_lock import digest_file, lock_from_wheels, verify_runtime


def test_valid_runtime_records_every_pin_and_actual_dependencies(runtime_config):
    receipt = verify_runtime(runtime_config)
    assert receipt['verified_libraries'] == runtime_config['libraries']
    assert receipt['loaded_libraries'] == runtime_config['libraries']
    assert receipt['verified_at_unix'] > 0


@pytest.mark.parametrize('change', ['tampered', 'missing', 'unlisted', 'empty', 'bad_hash', 'legacy'])
def test_invalid_runtime_refuses_before_loader(runtime_config, monkeypatch, change):
    library = Path(next(iter(runtime_config['libraries'])))
    if change == 'tampered':
        library.write_bytes(b'tampered library')
    elif change == 'missing':
        library.unlink()
    elif change == 'unlisted':
        del runtime_config['libraries'][runtime_config['environment']['ORT_DYLIB_PATH']]
    elif change == 'empty':
        runtime_config['libraries'] = {}
    elif change == 'bad_hash':
        runtime_config['libraries'][str(library)] = 'not-a-sha256'
    else:
        del runtime_config['libraries']
    monkeypatch.setattr(subprocess, 'run', lambda *a, **kw: pytest.fail('loader must not run'))
    with pytest.raises((ValueError, OSError)):
        verify_runtime(runtime_config)


@pytest.mark.parametrize('route', ['search_path', 'preload'])
def test_loader_detects_unlisted_shadow_even_with_identical_bytes(tmp_path, runtime_config, route):
    source = Path(next(iter(runtime_config['libraries'])))
    shadow = tmp_path / 'shadow'
    shadow.mkdir()
    shutil.copyfile(source, shadow / source.name)
    if route == 'search_path':
        runtime_config['environment']['LD_LIBRARY_PATH'] = (
            str(shadow) + ':' + runtime_config['environment']['LD_LIBRARY_PATH'])
    else:
        runtime_config['environment']['LD_PRELOAD'] = str(shadow / source.name)
    with pytest.raises(ValueError, match='unlisted loaded runtime'):
        verify_runtime(runtime_config)


def test_unlisted_cuda_dependency_refused(runtime_config):
    del runtime_config['libraries'][next(iter(runtime_config['libraries']))]
    with pytest.raises(ValueError, match='unlisted loaded runtime'):
        verify_runtime(runtime_config)


def test_broken_elf_is_not_a_valid_runtime(runtime_config):
    path = Path(runtime_config['environment']['ORT_DYLIB_PATH'])
    path.write_bytes(b'matches pin but cannot load')
    runtime_config['libraries'][str(path)] = digest_file(path)
    with pytest.raises(ValueError, match='loader verification failed'):
        verify_runtime(runtime_config)


def test_wheel_hashes_are_authority_not_installed_bytes(tmp_path, runtime_config):
    root = Path(runtime_config['environment']['ORT_DYLIB_PATH']).parent
    archive = tmp_path / 'runtime.whl'
    with zipfile.ZipFile(archive, 'w') as wheel:
        for path in runtime_config['libraries']:
            wheel.write(path, Path(path).name)
    legacy = {'ort_api': 23, 'environment': runtime_config['environment'],
              'artifacts': [{'filename': archive.name, 'sha256': digest_file(archive)}]}
    Path(next(iter(runtime_config['libraries']))).write_bytes(b'tampered')
    locked = lock_from_wheels(legacy, tmp_path, root)
    assert locked['libraries'] == runtime_config['libraries']
    with pytest.raises(ValueError, match='SHA256 mismatch'):
        verify_runtime(locked)
    archive.write_bytes(b'tampered archive')
    with pytest.raises(ValueError, match='artifact SHA256 mismatch'):
        lock_from_wheels(legacy, tmp_path, root)


def test_pilot_refuses_before_binary_lookup_preparation_or_spend(tmp_path, runtime_config, monkeypatch):
    config = tmp_path / 'runtime.json'
    Path(next(iter(runtime_config['libraries']))).write_bytes(b'tampered')
    config.write_text(json.dumps(runtime_config))
    out = tmp_path / 'run'
    monkeypatch.setattr(sys, 'argv', ['pilot', '--output', str(out), '--gpu-runtime', str(config)])
    for name in ('_find_bobbin', '_find_claude', 'prepare', 'setup_bobbin', 'invoke'):
        monkeypatch.setattr(pilot, name, lambda *a, **k: pytest.fail('task work must not start'))
    with pytest.raises(RuntimeError, match='SHA256 mismatch'):
        pilot.main()
    assert json.loads((out / 'runtime-verification.json').read_text())['status'] == 'refused'
    assert not (out / 'bin').exists()


def test_pilot_valid_runtime_recorded_before_preparation(tmp_path, runtime_config, monkeypatch):
    config = tmp_path / 'runtime.json'
    config.write_text(json.dumps(runtime_config))
    binary = tmp_path / 'fake-bobbin'
    binary.write_text('unused fixture')
    out = tmp_path / 'run'
    monkeypatch.setattr(sys, 'argv', ['pilot', '--output', str(out), '--gpu-runtime', str(config)])
    monkeypatch.setattr(pilot, '_find_bobbin', lambda: str(binary))
    monkeypatch.setattr(pilot, '_find_claude', lambda: 'unused-claude')
    monkeypatch.setattr(pilot, 'isolated_env', lambda _: {'PATH': '/usr/bin'})
    class Prepared(Exception):
        pass
    def stop(*a):
        raise Prepared
    monkeypatch.setattr(pilot, 'prepare', stop)
    with pytest.raises(Prepared):
        pilot.main()
    manifest = json.loads((out / 'manifest.json').read_text())
    assert manifest['runtime_verification']['status'] == 'verified'
    assert manifest['runtime_verification']['verified_libraries'] == runtime_config['libraries']
    assert manifest['runtime_verifier_sha256'] == digest_file(out / 'bin/runtime_lock.py')


def test_system_pin_traces_to_package_checksum_and_version(tmp_path, runtime_config, monkeypatch):
    import hashlib

    from runner.runtime_lock import package_pin, verified_files

    path = tmp_path / 'libcuda.so.1'
    path.write_bytes(b'packaged driver fixture')
    expected = hashlib.md5(path.read_bytes(), usedforsecurity=False).hexdigest()
    version = ['1.0-1']
    def dpkg(command, **kwargs):
        assert command[0] == 'dpkg-query'
        if command[1] == '-S':
            output = f'driver:amd64: {path}\n'
        elif command[1] == '-W':
            output = f'driver:amd64\t{version[0]}\tinstalled'
        else:
            assert command[1:] == ['--control-show', 'driver:amd64', 'md5sums']
            output = f'{expected}  {str(path).lstrip("/")}\n'
        return subprocess.CompletedProcess(command, 0, output, '')
    monkeypatch.setattr(subprocess, 'run', dpkg)
    pin = package_pin(path)
    assert pin == {'path': str(path), 'sha256': digest_file(path),
                   'package': 'driver:amd64', 'version': '1.0-1', 'package_md5': expected}
    runtime_config['system_packages'] = [pin]
    runtime_config['libraries'][str(path)] = pin['sha256']
    verified_files(runtime_config)
    version[0] = '1.0-2'
    with pytest.raises(ValueError, match='system package pin changed'):
        verified_files(runtime_config)
    path.write_bytes(b'changed driver fixture')
    with pytest.raises(ValueError, match='differs from package checksum'):
        package_pin(path)


def test_wrong_api_is_refused(runtime_config):
    runtime_config['ort_api'] = 24
    with pytest.raises(ValueError, match='requires the pinned ORT API 23'):
        verify_runtime(runtime_config)
