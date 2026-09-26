"""Transport/provenance tests; stub executable is not a hook-quality oracle."""

import json
import os
import sys

import pytest

from runner.hook_replay import capture_hook, main, sha256, validate_config


@pytest.fixture
def binary(tmp_path):
    path = tmp_path / "stub"
    path.write_text(
        f"#!{sys.executable}\n"
        + """
import json, os, pathlib, sys, urllib.request
assert "SHANTY_AGENT" not in os.environ
assert "BOBBIN_QUIPU_REMOTE" not in os.environ
assert "SECRET_TEST_TOKEN" not in os.environ
assert pathlib.Path.home().name == "home"
assert pathlib.Path.cwd().name == "repo"
incoming = json.load(sys.stdin)
assert incoming["session_id"] == "diagnostic"
url = sys.argv[2]
with urllib.request.urlopen(url + "/context?q=fixture") as r:
    value = json.load(r)
text = value.get("output", "")
if value.get("unexpected"):
    try: urllib.request.urlopen(url + "/search?q=fixture")
    except Exception: pass
if value.get("fail"):
    sys.exit(3)
if value.get("mutate"):
    with open(sys.argv[0], "a") as out: out.write("# changed\\n")
if text:
    data = json.dumps({"formatted_output": value.get("recorded", text)}).encode()
    urllib.request.urlopen(urllib.request.Request(url + "/injections", data=data)).close()
sys.stdout.write(text)
"""
    )
    path.chmod(0o700)
    return path


def run(binary, **kwargs):
    return capture_hook(
        binary=binary,
        expected_sha256=sha256(binary.read_bytes()),
        prompt="repair the parser",
        **kwargs,
    )


def test_real_process_and_loopback_roundtrip_isolated(binary, monkeypatch):
    for name in ("SHANTY_AGENT", "BOBBIN_QUIPU_REMOTE", "SECRET_TEST_TOKEN"):
        monkeypatch.setenv(name, "must-not-reach-child")
    prior = json.dumps({"chunk_key": "a:1:2", "injection_id": "old", "turn": 1}) + "\n"
    result = run(binary, response={"output": "source text\n"}, prior_ledger=prior)
    assert result["stdout"] == "source text\n"
    assert result["ledger_after"] == result["prior_ledger"] == prior
    assert result["requests"][0]["query"] == {"q": ["fixture"]}
    assert result["requests"][1]["body"]["formatted_output"] == result["stdout"]
    assert result["replay_ready"] is False


@pytest.mark.parametrize(
    "response,match",
    [
        ({"output": "a", "recorded": "b"}, "disagree"),
        ({"unexpected": True}, "unsupported endpoint"),
        ({"fail": True}, "exited 3"),
        ({"mutate": True}, "changed during"),
    ],
)
def test_refuses_false_success(binary, response, match):
    with pytest.raises(ValueError, match=match):
        run(binary, response=response)


def test_checksum_refuses_before_launch(binary):
    with pytest.raises(ValueError, match="checksum"):
        capture_hook(binary=binary, expected_sha256="wrong", prompt="x", response={})


@pytest.mark.parametrize(
    "config",
    [
        'quipu_endpoint="https://example.invalid"',
        '[hooks]\nkeywords=["a"]',
        '[plugins]\ncommand="touch somewhere"',
    ],
)
def test_config_excludes_external_and_structured_settings(config):
    with pytest.raises(ValueError, match="config"):
        validate_config(config)


def test_scalar_config_and_empty_output(binary):
    result = run(binary, response={}, config="[hooks]\ngate_threshold=0.45\nbudget=300\n")
    assert result["stdout"] == ""
    assert len(result["requests"]) == 1
    assert "why the hook skipped" in result["limitations"][-1]


def test_invalid_ledger_is_not_silently_ignored(binary):
    with pytest.raises(ValueError, match="ledger"):
        run(binary, response={}, prior_ledger='{"chunk_key":"a"}')


def test_cli_private_output_no_overwrite_and_failed_cleanup(binary, tmp_path, monkeypatch):
    response = tmp_path / "response.json"
    response.write_text('{"output":"capture"}')
    output = tmp_path / "out.json"
    argv = [
        "hook-replay",
        str(response),
        "--binary",
        str(binary),
        "--binary-sha256",
        sha256(binary.read_bytes()),
        "--prompt",
        "repair parser",
        "--out",
        str(output),
    ]
    monkeypatch.setattr(sys, "argv", argv)
    assert main() == 0
    before = output.read_bytes()
    if os.name == "posix":
        assert output.stat().st_mode & 0o777 == 0o600
    with pytest.raises(FileExistsError):
        main()
    assert output.read_bytes() == before
    output.unlink()
    response.write_text('{"fail":true}')
    with pytest.raises(ValueError, match="exited"):
        main()
    assert not output.exists()
