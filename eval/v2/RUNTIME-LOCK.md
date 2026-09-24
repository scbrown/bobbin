# L1 runtime lock

Before a new GPU pilot/study, prepare an approved library lock from the existing
artifact manifest. The historical manifest pins wheel archives; those hashes
alone do not establish the bytes in the extracted installation. From `eval/`:

```sh
python -m runner.runtime_lock lock \
  --runtime /path/to/runtime.json \
  --archives /path/to/downloads \
  --installed /path/to/extracted/lib \
  --output /path/to/new-runtime-lock.json
python -m runner.runtime_lock verify /path/to/new-runtime-lock.json
```

`lock` checks every archive SHA256 before deriving hashes from its shared-library
members. It does not hash the installation to choose expected values, download
anything, install packages, or overwrite an existing output. Review and freeze
the new lock before a run. Preserve historical campaign manifests and results.
For a system driver outside those wheels, add an explicit
`--system-library /usr/lib/example/libcuda.so.VERSION` to the lock command.
It must have a unique dpkg owner and an installed package version; its bytes
must match that package's `md5sums` record before SHA256 is pinned. This MD5
cross-check establishes package provenance, not the runtime integrity digest:
run-time checks use SHA256 and also reject changed package metadata. The receipt
keeps package/version/package-MD5/SHA256 in `system_packages`; no driver update
is performed. Review the generated pin. An NVIDIA package update requires a new
reviewed lock; silently accepting its replacement is forbidden.

The schema adds `library_lock_version: 1` and `libraries`, a nonempty mapping
from absolute library paths to lowercase SHA256 strings. Keep any additional
system/driver library pins under the same operator review; the verifier never
adds an unknown library automatically.

Pass the new file as `runner.pilot --gpu-runtime /path/to/new-runtime-lock.json`.
The runner refuses a legacy archive-only manifest, missing/changed libraries,
unpinned ORT/provider paths, loader errors, and unlisted loaded ORT/CUDA/NVIDIA
libraries **before preparing tasks or indexing**. Each paid cell and wrapped
Bobbin command rechecks the runtime; the wrapper checks after a gaming hold
clears. A refusal stops the campaign, rather than counting as a bad fixture.

The loader probe runs in a fresh Linux process with the command's library
environment. It uses the selected ORT API 23 to register the CUDA provider in disposable
session options, then reads `/proc/self/maps`. ORT initializes the shared-provider
host before loading CUDA; directly loading that provider can crash before its
constructor completes. See the [ORT provider bridge](https://github.com/microsoft/onnxruntime/blob/v1.23.2/onnxruntime/core/session/provider_bridge_ort.cc). The CUDA provider is
loaded before any absolute CUDA dependency can conceal a search-path shadow.
All pinned files are hashed, including optional TensorRT/Python libraries that
this CUDA probe does not load. An identical copy at an unlisted resolved path
is still refused. No model or GPU session is created by the probe.

`runtime-verification.json` records verified and loaded path/hash maps and the
verification time. The campaign manifest embeds the initial receipt and the
copied verifier's SHA256. Cells retain their own receipt; the wrapper records
checks under `bin/runtime-checks/`. A failed check writes a refusal receipt.

These are run-start checks of an operator-controlled runtime, not a signature
check or continuous audit of every future `dlopen`. Keep the runtime immutable
for the campaign: replacing it after a check can invalidate the observation.
The existing CUDA-session proof and gaming-hold gates still apply. This change
does not authorize full L1, resume an old campaign, or change its pins/limits.
