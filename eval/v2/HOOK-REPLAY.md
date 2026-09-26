# Frozen-response hook diagnostic

`runner.hook_replay` executes the pinned Bobbin binary's remote injection hook
against a loopback HTTP fixture. It exercises the actual gate, filters, session
ledger and renderer rather than maintaining Python copies of those algorithms.
It makes no model call, opens no index and changes no installed hook.

```sh
PYTHONPATH=eval python -m runner.hook_replay response.json \
  --binary /absolute/path/to/bobbin --binary-sha256 <sha256> \
  --prompt 'repair the parser error handling' --out hook-capture.json
```

`response.json` is the complete frozen `/context` response. A deliberately
synthetic response is also useful for discriminating controls. The baseline in
an assembly diagnostic is structurally compatible, but feeding it here does
**not** establish that the remote hook would have retrieved those candidates:
the remote request's query transformation and assembly policy can differ.
The artifact includes the actual request query parameters to make that mismatch
inspectable. Do not reconstruct the response from the pre-budget candidate pool.

Optional `--config` supplies a restricted TOML file: scalar hook `budget`,
`min_prompt_length`, `gate_threshold`, `reducing_enabled`, `show_docs`,
`format_mode`, and search `semantic_weight`, `doc_demotion`, `recency_weight`.
Other settings refuse; this is not a way to import a deployment's full config.
Optional `--ledger` supplies exact prior session JSONL bytes. Run a second capture
with the first artifact's `ledger_after` to test repeat-turn deduplication.

Each invocation creates a fresh private HOME and repo, with a fixed scratch
session identity. It excludes the caller's credentials, proxy settings, plate,
global config, tags and live session state. Governance and bundles are outside
this diagnostic's scope. This isolation is for the trusted Bobbin binary; it is
not an OS sandbox for executing untrusted programs.

The private output refuses overwrite and includes the binary checksum before
and after execution, input response/hash, config, prior/updated ledger, raw
stdout/stderr, and every loopback request. A recorded injection must exactly
match stdout. A fallback `/search`, other unexpected endpoint, changed binary,
nonzero exit or output disagreement refuses the capture. Empty stdout alone
does not identify the skip reason: inspect the request and ledger evidence.

`replay_ready` remains false. This does not attest a production turn, index/model
pins, role policy, repo affinity, useful facts, or a density/recall improvement.
It is a controlled hook-stage diagnostic for building the eventual paired
experiment. Independent labels and the preregistered Jev protocol remain required.

Validation covers transport, environment isolation, pin changes, unsupported
fallbacks, failed processes, malformed ledgers, exact output agreement and private
no-overwrite output. Those stub-process tests do not prove hook semantics. A
separate native run with the assembly-exporter binary exercised four arms:
nonempty output (GET + POST), same-session repeat (GET only, dedup skip), low raw
cosine (GET only, gate skip), and a short prompt (no HTTP request).
