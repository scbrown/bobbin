"""Run the actual remote hook against frozen responses in a scratch session.

This is a diagnostic, not a production capture or a Jev experiment. No ranking,
gate, deduplication, or rendering logic is reimplemented here.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
import threading
import tomllib
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


@contextmanager
def response_server(response: dict):
    """Only the two expected hook endpoints exist; record every request."""
    requests: list[dict] = []
    payload = json.dumps(response, allow_nan=False).encode()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def answer(self, status: int, body: bytes):
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            url = urlsplit(self.path)
            requests.append({"method": "GET", "path": url.path, "query": parse_qs(url.query)})
            self.answer(
                200 if url.path == "/context" else 404, payload if url.path == "/context" else b"{}"
            )

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= 2_000_000:
                requests.append({"method": "POST", "path": self.path, "invalid_size": size})
                self.answer(400, b"{}")
                return
            body = json.loads(self.rfile.read(size))
            requests.append({"method": "POST", "path": self.path, "body": body})
            self.answer(200 if self.path == "/injections" else 404, b"{}")

    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def validate_config(config: str) -> None:
    # Accept only scalar hook/search settings. In particular do not import an
    # endpoint, a plugin command, global tags, credentials, or a live index.
    parsed = tomllib.loads(config)
    if set(parsed) - {"hooks", "search"}:
        raise ValueError("diagnostic config accepts only hooks and search sections")
    allowed = {
        "hooks": {
            "budget": int,
            "min_prompt_length": int,
            "gate_threshold": float,
            "reducing_enabled": bool,
            "show_docs": bool,
            "format_mode": str,
        },
        "search": {
            "semantic_weight": float,
            "doc_demotion": float,
            "recency_weight": float,
        },
    }
    for section, values in parsed.items():
        if not isinstance(values, dict):
            raise ValueError("diagnostic config requires tables")
        for key, value in values.items():
            expected = allowed[section].get(key)
            if expected is None or type(value) is not expected:
                raise ValueError("diagnostic config contains an unsupported key or type")
            if expected is int and value < 0:
                raise ValueError("diagnostic config requires nonnegative counts")
            if expected is float and not 0 <= value <= 1:
                raise ValueError("diagnostic config requires probabilities in [0,1]")
            if key == "format_mode" and value not in {"standard", "minimal", "verbose", "xml"}:
                raise ValueError("diagnostic config contains an unknown output format")


def capture_hook(
    *,
    binary: Path,
    expected_sha256: str,
    prompt: str,
    response: dict,
    config: str = "",
    prior_ledger: str = "",
    timeout: float = 30,
) -> dict:
    """Capture a single real hook invocation; never touch a caller's session.

    The caller supplies the original /context response (or a deliberately
    synthetic fixture). It is NOT reconstructed from pre-budget candidates.
    """
    binary = binary.resolve(strict=True)
    binary_bytes = binary.read_bytes()
    if sha256(binary_bytes) != expected_sha256:
        raise ValueError("binary checksum differs from the frozen pin")
    validate_config(config)
    # Validate ledger input instead of silently letting the hook ignore bad rows.
    for line in prior_ledger.splitlines():
        row = json.loads(line)
        if (
            set(row) != {"chunk_key", "injection_id", "turn"}
            or not isinstance(row["chunk_key"], str)
            or not isinstance(row["injection_id"], str)
            or type(row["turn"]) is not int
            or row["turn"] < 0
        ):
            raise ValueError("invalid prior ledger row")
    response_bytes = json.dumps(response, sort_keys=True, allow_nan=False).encode()
    with tempfile.TemporaryDirectory(prefix="bobbin-hook-replay-") as tmp:
        root = Path(tmp)
        home = root / "home"
        repo = root / "repo"
        home.mkdir()
        data = repo / ".bobbin"
        session = data / "session" / "diagnostic"
        session.mkdir(parents=True)
        (data / "config.toml").write_text(config)
        ledger = session / "ledger.jsonl"
        ledger.write_text(prior_ledger)
        # No inherited plate, provider keys, proxy, global config or endpoints.
        env = {
            "PATH": os.defpath,
            "HOME": str(home),
            "TMPDIR": str(root),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "XDG_CACHE_HOME": str(home / ".cache"),
            "XDG_DATA_HOME": str(home / ".local/share"),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_SYSTEM": os.devnull,
            "BOBBIN_GPU": "0",
        }
        with response_server(response) as (url, requests):
            result = subprocess.run(
                [str(binary), "--server", url, "hook", "inject-context"],
                input=json.dumps({"prompt": prompt, "cwd": str(repo), "session_id": "diagnostic"}),
                text=True,
                check=False,
                capture_output=True,
                cwd=repo,
                env=env,
                timeout=timeout,
            )
        if sha256(binary.read_bytes()) != expected_sha256:
            raise ValueError("binary changed during the invocation")
        unexpected = [
            r
            for r in requests
            if (r["method"], r["path"]) not in {("GET", "/context"), ("POST", "/injections")}
            or "invalid_size" in r
        ]
        if unexpected:
            raise ValueError("hook requested an unsupported endpoint; no successful replay")
        posts = [r["body"] for r in requests if r["method"] == "POST"]
        if result.returncode != 0:
            raise ValueError(f"hook exited {result.returncode}; no successful replay")
        if result.stdout and (len(posts) != 1 or posts[0].get("formatted_output") != result.stdout):
            raise ValueError("hook stdout and recorded injection disagree")
        return {
            "schema": "bobbin-hook-replay-v1",
            "scope": "frozen-response-diagnostic",
            "binary_sha256": expected_sha256,
            "response_sha256": sha256(response_bytes),
            "response": response,
            "prompt": prompt,
            "config": config,
            "prior_ledger": prior_ledger,
            "ledger_after": ledger.read_text(),
            "stdout": result.stdout,
            "stderr": result.stderr,
            "requests": requests,
            "exit_code": result.returncode,
            "replay_ready": False,
            "limitations": [
                "Frozen /context response; no live retrieval or index snapshot attested.",
                "Scratch session excludes plate, bundles, governance and repo affinity.",
                "No Jev call, human labels, gold scoring or measured quality gain.",
                "Empty stdout alone does not identify why the hook skipped.",
            ],
        }


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("response", type=Path)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--ledger", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    # Reserve the output before executing. Never overwrite an existing capture.
    fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        artifact = capture_hook(
            binary=args.binary,
            expected_sha256=args.binary_sha256,
            prompt=args.prompt,
            response=json.loads(args.response.read_text()),
            config=args.config.read_text() if args.config else "",
            prior_ledger=args.ledger.read_text() if args.ledger else "",
        )
        with os.fdopen(fd, "w") as output:
            fd = -1
            json.dump(artifact, output, indent=2, allow_nan=False)
            output.write("\n")
    except BaseException:
        if fd >= 0:
            os.close(fd)
        args.out.unlink()
        raise
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
