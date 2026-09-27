#!/usr/bin/env python3
"""Real CLI + local HTTP recorder: unchanged snapshots send zero requests.

Requires cached embedding model and ONNX runtime. Uses only temporary stores and
an HTTP server bound to loopback; never reads a production bearer or graph.
"""
import json
import os
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import subprocess
import sys
import tempfile
import threading


def main():
    calls = []
    uploads = {}
    failing = False

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            calls.append(self.path)
            status = 400 if failing else 200
            if self.path == '/knot/stage':
                uploads[body['upload_id']] = body['content_hash']
                response = {'staged': True}
            elif self.path == '/knot/promote':
                response = {'promoted': True, 'replaced': True, 'conforms': True,
                            'content_hash': uploads[body['upload_id']],
                            'tx_id': len(calls), 'count': 10}
            else:
                status, response = 404, {'error': 'unexpected request'}
            payload = json.dumps(response).encode()
            self.send_response(status)
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    binary = str(Path(sys.argv[1]).resolve())
    try:
        with tempfile.TemporaryDirectory(prefix='bobbin-publish-dedup-') as tmp:
            root = Path(tmp)
            (root / '.bobbin').mkdir()
            (root / '.bobbin/config.toml').write_text(f'''quipu_push_chunks = true
quipu_endpoint = "http://127.0.0.1:{server.server_port}"
[git]
commits_enabled = false
[embedding]
gpu = false
''')
            source = root / 'a.rs'
            source.write_text('pub fn alpha() -> usize { 1 }\n')
            env = dict(os.environ, QUIPU_AUTH_TOKEN='isolated-test-token')
            env.pop('BOBBIN_SERVER', None)

            def index(success=True, extra=()):
                p = subprocess.run([binary, 'index', str(root), '--repo', 'fixture',
                                    '--skip-calibrate', *extra], env=env, capture_output=True,
                                   text=True, timeout=120)
                assert (p.returncode == 0) == success, p.stdout[-1000:] + p.stderr[-1000:]

            index()
            before = len(calls)
            assert before > 0 and '/knot/promote' in calls, 'publication control'
            index()
            print(f'unchanged-run HTTP requests: {len(calls) - before}', flush=True)
            assert len(calls) == before, 'unchanged run must send zero requests'
            source.write_text('pub fn beta() -> usize { 1 }\n')
            failing = True
            index(success=False)
            failed = len(calls)
            assert failed > before, 'changed snapshot must attempt publication'
            failing = False
            index()
            assert len(calls) > failed, 'failed snapshot must retry without a source edit'
            successful = len(calls)
            index()
            assert len(calls) == successful, 'successful retry hash must persist across processes'
            index(extra=('--force',))
            assert len(calls) > successful, 'explicit recovery must bypass the success hash'
            forced = len(calls)
            # A new destination must not inherit another server's success hash.
            config = root / '.bobbin/config.toml'
            config.write_text(config.read_text().replace(
                f':{server.server_port}"', f':{server.server_port}/other"'))
            index(success=False)
            assert len(calls) > forced, 'destination change must attempt publication'
            print('PASS: zero unchanged requests; retry persistence; force and destination isolation')
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == '__main__':
    main()
