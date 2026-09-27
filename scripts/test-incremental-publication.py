#!/usr/bin/env python3
"""Exercise the real index command twice; unchanged graph entities must survive.

Usage: python3 scripts/test-incremental-publication.py /path/to/bobbin
Requires the usual cached embedding model and ONNX runtime; never calls a remote
Quipu or uses the user's graph. Both index and graph live in a temporary directory.
"""
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile


def active_alpha(root):
    with sqlite3.connect(root / '.bobbin/quipu/quipu.db') as db:
        return db.execute('''SELECT count(*) FROM facts f
            JOIN terms e ON e.id=f.e JOIN terms a ON a.id=f.a
            WHERE e.iri LIKE '%/a.rs::alpha'
              AND a.iri='http://www.w3.org/1999/02/22-rdf-syntax-ns#type'
              AND f.op=1 AND f.valid_to IS NULL''').fetchone()[0]


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='bobbin-publication-') as tmp:
        root = Path(tmp)
        (root / '.bobbin').mkdir()
        (root / '.bobbin/config.toml').write_text('''quipu_push_chunks = true
[git]
commits_enabled = false
[embedding]
gpu = false
''')
        (root / 'a.rs').write_text('pub fn alpha() -> usize { 1 }\n')
        (root / 'b.rs').write_text('pub fn beta() -> usize { 2 }\n')
        env = dict(os.environ)
        env.pop('BOBBIN_SERVER', None)
        def index():
            p = subprocess.run([binary, 'index', str(root), '--repo', 'fixture',
                                '--skip-calibrate'], env=env, capture_output=True, text=True)
            if p.returncode:
                print(p.stdout[-2000:], p.stderr[-2000:])
                raise RuntimeError(f'index failed with {p.returncode}')
        index()
        before = active_alpha(root)
        assert before > 0, 'positive control: first full publication must type alpha'
        (root / 'b.rs').write_text('pub fn beta() -> usize { 3 }\n')
        index()
        after = active_alpha(root)
        print(f'unchanged alpha type assertions: {before} -> {after}', flush=True)
        assert after == before, 'incremental publication retracted unchanged a.rs'
        index()
        assert active_alpha(root) == before, 'no-change publication lost alpha'
        print('PASS: changed-file and no-change publication retain alpha')


if __name__ == '__main__':
    main()
