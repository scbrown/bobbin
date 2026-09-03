# pack

`bobbin pack` moves a complete, prebuilt local index between checkouts. A pack includes the
LanceDB tables (vectors, FTS, dependency and chunk-edge state), the SQLite metadata/file hashes
and temporal coupling graph, and a versioned manifest.

```bash
bobbin pack export . --repo my-repo --source ../my-repo -o my-repo.bbpack
bobbin pack verify my-repo.bbpack --path .
bobbin pack import my-repo.bbpack --path . --source ../my-repo
```

The manifest schema is `https://github.com/scbrown/bobbin/index-pack/v1`. It binds the payload to
the repository name and Git SHA; embedding backend, model ID, model version and dimensions;
Bobbin build/version, SQLite version and Lance format version; and a SHA-256 for every payload
file. `verify` refuses checksum, schema, or embedding-identity mismatches.

Import extracts into a staging directory inside `.bobbin`, verifies before activation, confirms
the packed SHA exists and is an ancestor of the checkout's HEAD, then swaps both stores together.
If activation fails, the previous stores are restored. An invalid candidate passed to `import` is
deleted and never activated. Unless `--no-reindex` is explicitly supplied, import finishes by
running Bobbin's normal hash- and watermark-based incremental index from the packed state to HEAD.

## One repository per pack

Build an artifact from a dedicated per-repository Bobbin home. Export refuses a Lance store that
contains another repository. This is required because the SQLite temporal-coupling table is not
repository-scoped; filtering only the vector rows would make a pack look valid while carrying the
wrong coupling graph.

## Storage policy

Do not commit large packs into ordinary Git history. Choose transport from the measured compressed
artifact size:

| Compressed pack | Distribution |
|---|---|
| up to 10 MiB | May be committed in-repo when clone cost is intentional |
| over 10 MiB through 100 MiB | Git LFS |
| over 100 MiB | Release asset |

Record the measured size for each repository when publishing. A current local Bobbin-only store
measured 65,162,206 payload bytes and compressed to 28 MiB, placing it in the Git LFS tier. Quipu,
Shantytown and Yupana did not have dedicated local Bobbin indexes at the time this policy was
written, so their sizes remain to be measured from actual per-repo packs rather than extrapolated.

Because packs contain source text and metadata, publish them with the same exposure classification
as the repository they index.
