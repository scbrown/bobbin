# Local assembly diagnostic capture

Use an isolated, frozen index to inspect the actual input to budget packing:

```sh
bobbin context 'example task' ./fixture --content full --budget 300 --limit 30 \
  --capture-assembly ./capture.json
```

This opt-in command writes one private JSON file (mode 0600 on Unix) and refuses
an existing output. It makes no model call and changes neither ranking nor normal
command output. The artifact contains query and source text: keep it with private
evaluation inputs. Nothing is recorded by default or by the production hook.

The snapshot is taken after seed path deduplication, noise filtering and feedback
adjustment, before partitioning or budget packing. Expansion streams use the same
noise predicate as assembly. All eligible direct, pinned, coupled, bridged,
knowledge and structural candidates are retained, including candidates omitted by
the budget. Duplicate IDs across streams are intentional: assembly resolves them
in phase order. Their scores have different meanings, recorded in `score_kind`;
the array is not a single globally sorted ranking. `--limit` limits initial search
results, not the combined snapshot. No top-30 truncation occurs.

The baseline comes from the same assembly call and passes the same local role
filter as the captured candidates. The artifact records the running binary hash,
build commit/dirty marker, checkout commit/dirty marker when Git is available,
embedding model, effective assembly policy and access role. Unknown checkout
metadata stays null. The index is explicitly **not pinned** by this command;
provenance does not certify index contents or model file hashes.

This is the local `context` path, **not a captured production hook turn**.
`hook_gate` and `session_dedup` are `not_run`; `replay_ready` is false. Local context
configuration can differ from remote hook configuration. The baseline is a
`ContextBundle`, not the replay module's scored `ArmScore`. Preview/none output
modes remain visible in the policy; use full mode for source-text inspection.

Do not feed this artifact straight to `runner.jev_replay`, claim that it captures
post-hook output, or reconstruct a production baseline from the candidate list.
The remaining experiment adapter must freeze the index/model bytes and hook state,
capture the actual gate/dedup/rendered baseline, preserve pinned/expanded content,
declare candidate selection and score normalization, and join gold only after
capture. Human labels, a model/client revision and a preregistered protocol remain
gates before a model-assisted run. This diagnostic makes those inputs inspectable;
it is not evidence of a density or recall improvement.

For an isolated execution of the actual remote hook against a frozen response,
see [HOOK-REPLAY.md](HOOK-REPLAY.md). It exposes gate/dedup/render behavior but
does not turn a local assembly capture into a production retrieval capture.
