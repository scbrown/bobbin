#!/usr/bin/env python3
"""Prove the configured private-key hook refuses, then scan tracked files.

Requires pre-commit. Synthetic markers are created only in a temporary repository;
no real key is generated, registered, staged or committed by this self-test.
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def run(root, *args):
    return subprocess.run(['pre-commit', 'run', 'detect-private-key', *args],
                          cwd=root, text=True, capture_output=True)


def selftest(root):
    with tempfile.TemporaryDirectory(prefix='private-key-hook-test-') as directory:
        probe = Path(directory)
        subprocess.run(['git', 'init', '--quiet', str(probe)], check=True)
        shutil.copyfile(root / '.pre-commit-config.yaml', probe / '.pre-commit-config.yaml')
        candidate = probe / 'ordinary-notes.txt'
        candidate.write_text('Public documentation only.\n')
        benign = run(probe, '--files', candidate.name)
        if benign.returncode != 0 or 'Passed' not in benign.stdout:
            raise RuntimeError('benign control did not run and pass: ' + benign.stdout + benign.stderr)
        # Split markers so the test source itself does not contain key material.
        for kind in ('PRIVATE KEY', 'RSA PRIVATE KEY', 'OPENSSH PRIVATE KEY'):
            candidate.write_text('-----BEGIN ' + kind + '-----\nsynthetic-not-a-key\n')
            refused = run(probe, '--files', candidate.name)
            if refused.returncode != 1 or 'Private key found:' not in refused.stdout:
                raise RuntimeError('private-key refusal did not run: ' + refused.stdout + refused.stderr)
    print('PASS: benign control and three private-key refusals; no probe staged')


def main():
    root = Path(__file__).resolve().parents[1]
    selftest(root)
    if '--selftest' not in sys.argv:
        result = run(root, '--all-files')
        # The upstream hook prints filenames only, never matching key bytes.
        print(result.stdout, end='')
        print(result.stderr, end='', file=sys.stderr)
        return result.returncode
    return 0


if __name__ == '__main__':
    sys.exit(main())
