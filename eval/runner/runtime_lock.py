"""Linux ORT/CUDA run-start lock verification; no indexing or GPU sessions.

The lock is operator-approved input, not a signature or a lifetime integrity
monitor. Hashes come from verified wheel contents, never from installed files.
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import zipfile
from pathlib import Path

# Include the driver if the loader brings it in, but never install/change it.
RUNTIME = re.compile(r"^(?:libonnxruntime|onnxruntime_pybind|libcu(?:da|blas|dnn|fft|rand|solver|sparse)|libnv|libnccl).*\.so(?:\.|$)")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def digest_file(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def lock_from_wheels(config, archives, installed):
    """Produce a new lock using the legacy manifest's trusted archive hashes."""
    if not config.get('artifacts'):
        raise ValueError('runtime lock requires pinned artifacts')
    libraries = {}
    installed = installed.resolve(strict=True)
    for artifact in config['artifacts']:
        filename = artifact['filename']
        if Path(filename).name != filename or not SHA256.fullmatch(artifact['sha256']):
            raise ValueError('invalid pinned artifact')
        archive = archives / filename
        if digest_file(archive) != artifact['sha256']:
            raise ValueError(f'artifact SHA256 mismatch: {filename}')
        with zipfile.ZipFile(archive) as wheel:
            for member in wheel.namelist():
                if not RUNTIME.match(Path(member).name):
                    continue
                path = installed / member
                if Path(member).is_absolute() or '..' in Path(member).parts:
                    raise ValueError(f'unsafe wheel member: {member}')
                with wheel.open(member) as stream:
                    sha = hashlib.file_digest(stream, 'sha256').hexdigest()
                if str(path) in libraries:
                    raise ValueError(f'duplicate library: {member}')
                libraries[str(path)] = sha
    if not libraries:
        raise ValueError('artifacts contain no runtime libraries')
    return {**config, 'library_lock_version': 1, 'libraries': libraries}


def package_pin(path):
    """Explicit system-library pin backed by dpkg's installed package record."""
    path = Path(path).resolve(strict=True)

    def query(*args):
        return subprocess.run(['dpkg-query', *args], check=True, text=True,
                              capture_output=True, timeout=10).stdout.strip()

    ownership = query('-S', str(path)).splitlines()
    if len(ownership) != 1:
        raise ValueError(f'ambiguous package ownership: {path}')
    package, separator, owned = ownership[0].rpartition(': ')
    if not separator or owned != str(path):
        raise ValueError(f'package does not own library: {path}')
    metadata = query('-W', '-f=${binary:Package}\t${Version}\t${db:Status-Status}', package)
    name, version, status = metadata.split('\t')
    if name != package or status != 'installed':
        raise ValueError(f'package is not installed: {package}')
    checksums = [line.split(maxsplit=1) for line in query('--control-show', package, 'md5sums').splitlines()]
    expected = [parts[0] for parts in checksums
                if len(parts) == 2 and parts[1] == str(path).lstrip('/')]
    if len(expected) != 1:
        raise ValueError(f'no unique package checksum: {path}')
    sha, md5 = hashlib.sha256(), hashlib.md5(usedforsecurity=False)
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            sha.update(block)
            md5.update(block)
    if md5.hexdigest() != expected[0]:
        raise ValueError(f'installed library differs from package checksum: {path}')
    return {'path': str(path), 'sha256': sha.hexdigest(), 'package': package,
            'version': version, 'package_md5': expected[0]}


def verified_files(config):
    libraries = config.get('libraries')
    if config.get('library_lock_version') != 1 or not isinstance(libraries, dict) or not libraries:
        raise ValueError('runtime requires a version 1 library lock; artifact hashes alone are insufficient')
    verified = {}
    for name, expected in libraries.items():
        path = Path(name)
        if not path.is_absolute() or not isinstance(expected, str) or not SHA256.fullmatch(expected):
            raise ValueError(f'invalid library pin: {name}')
        resolved = str(path.resolve(strict=True))
        if not path.is_file() or digest_file(path) != expected:
            raise ValueError(f'library SHA256 mismatch: {name}')
        if resolved in verified:
            raise ValueError(f'duplicate resolved library: {name}')
        verified[resolved] = expected
    ort = Path(config['environment']['ORT_DYLIB_PATH']).resolve(strict=True)
    required = [ort, ort.parent / 'libonnxruntime_providers_shared.so',
                ort.parent / 'libonnxruntime_providers_cuda.so']
    for path in required:
        if str(path.resolve(strict=True)) not in verified:
            raise ValueError(f'unlisted ORT library: {path}')
    for pin in config.get('system_packages', []):
        if verified.get(pin['path']) != pin['sha256'] or package_pin(pin['path']) != pin:
            raise ValueError(f'system package pin changed: {pin["path"]}')
    return verified, required


def mapped_runtime():
    """Inspect actual mappings, not ldd's prediction or a directory listing."""
    result = set()
    for line in Path('/proc/self/maps').read_text().splitlines():
        fields = line.split(maxsplit=5)
        if len(fields) != 6 or not fields[5].startswith('/'):
            continue
        name = fields[5]
        if RUNTIME.match(Path(name.removesuffix(' (deleted)')).name):
            if name.endswith(' (deleted)'):
                raise ValueError(f'loaded runtime library was deleted: {name}')
            result.add(str(Path(name).resolve(strict=True)))
    return result


def load_cuda_provider(ort_path, api_version):
    """Use ORT's public API so its provider host is set before CUDA dlopen.

    ABI: v1.23.2 include/onnxruntime/core/session/onnxruntime_c_api.h,
    OrtApiBase and the first 11 OrtApi entries (through CreateSessionOptions).
    No session or execution provider instance is created. The options/factory
    live until this disposable probe process exits.
    """
    if api_version != 23:
        raise ValueError('loader probe requires the pinned ORT API 23')
    ptr = ctypes.c_void_p
    ort = ctypes.CDLL(str(ort_path), mode=os.RTLD_NOW | os.RTLD_LOCAL)
    ort.OrtGetApiBase.argtypes = []
    ort.OrtGetApiBase.restype = ctypes.POINTER(ptr)
    base = ort.OrtGetApiBase()
    if not base:
        raise ValueError('ORT returned no API base')
    api = ctypes.CFUNCTYPE(ctypes.POINTER(ptr), ctypes.c_uint32)(base[0])(api_version)
    if not api:
        raise ValueError('ORT does not support the pinned API')
    create_options = ctypes.CFUNCTYPE(ptr, ctypes.POINTER(ptr))(api[10])
    error_message = ctypes.CFUNCTYPE(ctypes.c_char_p, ptr)(api[2])
    options = ptr()

    def check(status):
        if status:
            raise ValueError(f'ORT provider load failed: {error_message(status).decode()}')

    check(create_options(ctypes.byref(options)))
    if not options:
        raise ValueError('ORT returned no session options')
    append = ort.OrtSessionOptionsAppendExecutionProvider_CUDA
    append.argtypes, append.restype = [ptr, ctypes.c_int], ptr
    check(append(options, 0))
    return ort


def probe(config):
    """Fresh loader, same library environment, no model/session or CUDA work."""
    verified, required = verified_files(config)
    # Load the provider BEFORE the other absolute paths: preloading them could
    # conceal a shadow dependency selected by LD_LIBRARY_PATH or an ELF RPATH.
    ort = load_cuda_provider(required[0], config.get('ort_api'))
    loaded = mapped_runtime()
    unknown = loaded - verified.keys()
    if unknown:
        raise ValueError(f'unlisted loaded runtime libraries: {sorted(unknown)}')
    # Rehash after loading as well, catching a replacement during the probe.
    after, _ = verified_files(config)
    if after != verified:
        raise ValueError('runtime changed during loader probe')
    if not {str(p.resolve(strict=True)) for p in required} <= loaded:
        raise ValueError('loader probe did not map every required ORT library')
    del ort  # CDLL keeps its native handle; the probe process owns its lifetime.
    return {'verified_libraries': verified,
            'loaded_libraries': {p: verified[p] for p in sorted(loaded)}}


def verify_runtime(config):
    # Parent check precedes any dlopen. The fresh process is necessary because
    # changing LD_LIBRARY_PATH in an existing Python process is not sufficient.
    verified_files(config)
    env = dict(os.environ, **config['environment'])
    env['BOBBIN_GPU'] = '1'
    result = subprocess.run([sys.executable, '-I', '-X', 'faulthandler',
                             str(Path(__file__).resolve()), 'probe'],
                            input=json.dumps(config), env=env, capture_output=True,
                            text=True, timeout=120, check=False)
    if result.returncode != 0:
        raise ValueError(f'runtime loader verification failed (exit {result.returncode}): '
                         f'{result.stderr.strip()}')
    receipt = json.loads(result.stdout)
    if not receipt.get('loaded_libraries') or not receipt.get('verified_libraries'):
        raise ValueError('runtime loader returned no verification evidence')
    return {'verified_at_unix': time.time(),
            'system_packages': config.get('system_packages', []), **receipt}


def verify_and_record(config, path):
    """A refusal is a campaign error, never a skippable fixture failure."""
    try:
        receipt = {'status': 'verified', **verify_runtime(config)}
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as exc:
        path.write_text(json.dumps({'status': 'refused', 'error': str(exc)}) + '\n')
        raise RuntimeError(f'runtime lock refused: {exc}') from exc
    path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('probe')  # internal: config from stdin
    lock = commands.add_parser('lock')
    lock.add_argument('--runtime', type=Path, required=True)
    lock.add_argument('--archives', type=Path, required=True)
    lock.add_argument('--installed', type=Path, required=True)
    lock.add_argument('--output', type=Path, required=True)
    lock.add_argument('--system-library', type=Path, action='append', default=[],
                      help='Explicit additional library to pin from its dpkg package record')
    verify = commands.add_parser('verify')
    verify.add_argument('runtime', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'probe':
            print(json.dumps(probe(json.load(sys.stdin))))
        elif args.command == 'lock':
            config = lock_from_wheels(json.loads(args.runtime.read_text()), args.archives, args.installed)
            config['system_packages'] = []
            for path in args.system_library:
                pin = package_pin(path)
                if pin['path'] in config['libraries']:
                    raise ValueError(f'duplicate system library: {path}')
                config['libraries'][pin['path']] = pin['sha256']
                config['system_packages'].append(pin)
            # Exclusive create: never rewrite an existing campaign/runtime lock.
            with args.output.open('x') as stream:
                json.dump(config, stream, indent=2, sort_keys=True)
                stream.write('\n')
        else:
            print(json.dumps(verify_runtime(json.loads(args.runtime.read_text())), indent=2))
    except (OSError, ValueError, KeyError, TypeError, AttributeError,
            subprocess.SubprocessError, zipfile.BadZipFile) as exc:
        print(f'runtime lock refused: {exc}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
