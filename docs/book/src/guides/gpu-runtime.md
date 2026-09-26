# Automatic GPU runtime for indexing

On Linux x86_64, `bobbin index` detects an NVIDIA GPU through `nvidia-smi`.
For the local ONNX backend, it installs a pinned ONNX Runtime 1.23.2 and CUDA 12
user space runtime in Bobbin's user data directory when a CUDA runtime is missing.
It never installs drivers, changes kernel modules, or runs a package manager.
The first download is large; allow several GB of free disk space.

Downloads use HTTPS and the SHA-256 pins in `src/gpu_runtime/artifacts.json`.
Archives are verified before extraction. An installation lock and staging directory
prevent parallel commands from using a partial installation. Installed libraries
are checked against their receipt on subsequent automatic cache loads. Damaged
installations are retained as `.damaged-*` directories and replaced atomically.
Licenses from each wheel are retained beside the libraries.

The process restarts once with the native library search path set, before loading
ONNX Runtime. A successfully registered CUDA session logs:

```text
ONNX session using CUDA GPU acceleration
```

Detection alone is not proof of acceleration. Registration errors fail explicitly;
download errors report their cause and select CPU. `bobbin status` (including
`--json`) reports device, runtime discovery, opt-out and hold state without installing
anything. Remote status does not substitute the client's GPU for the server's GPU.

## Controls

- `BOBBIN_GPU=0` forces CPU and suppresses GPU downloads.
- `BOBBIN_GPU_CACHE` selects the runtime cache root.
- An explicit `ORT_DYLIB_PATH` pointing at a CUDA runtime remains authoritative.
  Its CUDA dependencies must already be available to the native loader.
- API embedding backends and remote index commands never provision local CUDA.
- Other operating systems retain their existing runtime path; automatic downloading
  currently supports Linux x86_64. Other commands can use the installed runtime by
  supplying its library path and native loader path explicitly.

## Shared GPUs

Set `BOBBIN_GPU_HOLD_FILE` to a file whose presence means GPU work must yield, or
set `BOBBIN_GPU_HOLD_COMMAND` to a JSON argument array. For example:

```sh
export BOBBIN_GPU_HOLD_COMMAND='["my-gaming-check", "--status"]'
```

The command must return zero only when clear. Nonzero, malformed, missing or timed-out
probes are treated as held. It is executed directly, without a shell. On a managed
Shantytown host (`SHANTY_ROOT` set), the default probe is `st --root <root> fleet hold
gaming --status`. An explicit command overrides that default.

A hold present at index startup selects CPU without downloading. A hold that appears
after a CUDA session starts pauses before the next bounded embedding batch and resumes
when clear. File holds are checked before every batch. A successful command probe is
cached for at most one second to avoid launching a policy CLI for every fast batch;
command holds can therefore take that interval plus the current batch to be observed.
The current batch may finish, and the paused session retains its GPU memory.
This is cooperative scheduling, not a GPU-memory eviction mechanism.
