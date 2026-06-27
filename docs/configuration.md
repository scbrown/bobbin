# Configuration

Bobbin stores its configuration in `.bobbin/config.toml`, created by `bobbin init`.

## Full Default Configuration

```toml
[index]
# Glob patterns for files to include
include = [
    "**/*.rs",
    "**/*.ts",
    "**/*.tsx",
    "**/*.js",
    "**/*.jsx",
    "**/*.py",
    "**/*.go",
    "**/*.java",
    "**/*.cpp",
    "**/*.cc",
    "**/*.hpp",
    "**/*.md",
]

# Glob patterns for files to exclude (in addition to .gitignore)
exclude = [
    "**/node_modules/**",
    "**/target/**",
    "**/dist/**",
    "**/.git/**",
    "**/build/**",
    "**/__pycache__/**",
]

# Whether to respect .gitignore files
use_gitignore = true

[embedding]
# Embedding model (downloaded automatically on first run)
model = "all-MiniLM-L6-v2"

# Batch size for embedding generation
batch_size = 32

[embedding.context]
# Number of context lines to include before and after a chunk
# when generating its embedding. More context improves retrieval
# quality at the cost of slightly longer indexing time.
context_lines = 5

# Languages where contextual embedding is enabled.
# Contextual embedding enriches each chunk with surrounding
# lines before computing its vector, improving search relevance.
enabled_languages = ["markdown"]

[search]
# Default number of search results
default_limit = 10

# Weight for semantic vs keyword search in hybrid mode.
# 0.0 = keyword only, 1.0 = semantic only, default 0.9.
semantic_weight = 0.9

[hooks]
# Per-result filter on normalized RRF scores
threshold = 0.5

# Max lines of injected context
budget = 300

# Content display mode: full | preview | none
content_mode = "full"

# Skip injection for prompts shorter than this
min_prompt_length = 20

# Min raw semantic similarity to inject at all.
# The top semantic result's cosine similarity (before RRF normalization)
# must exceed this value, or the entire injection is skipped.
gate_threshold = 0.45

# Skip injection when search results haven't changed since last prompt.
# Uses a session ID derived from the top-10 chunk keys.
dedup_enabled = true

[git]
# Enable temporal coupling analysis (tracks which files
# frequently change together in git history)
coupling_enabled = true

# Number of commits to analyze for coupling relationships
coupling_depth = 5000

# Minimum number of co-changes required to establish a coupling link
coupling_threshold = 3

[context]
# Score multiplier for bridge-boosted files: final_score *= (1.0 + factor).
# Only used in boost / boost_inject bridge modes.
bridge_boost_factor = 0.3

# Max bridged files to fetch chunks for (prevents doc->source bridge explosion)
max_bridged_files = 2

# Max chunks kept per bridged file (first N by start line)
max_bridged_chunks_per_file = 1

# Minimum temporal-coupling SCORE (float 0.0-1.0) for a coupled file to enter
# context. Distinct from [git].coupling_threshold (an integer co-change COUNT).
coupling_threshold = 0.1

# Percent of the line budget reserved for knowledge-graph expansion
# (requires the `knowledge` feature build)
knowledge_budget_pct = 15.0

# Max graph-traversal hops for knowledge expansion
knowledge_max_hops = 2

[feedback]
# Maximum feedback boost multiplier. Actual boost = min(score * boost_weight, boost_max).
boost_max = 0.3

# Weight multiplier applied to raw cross-agent feedback scores
boost_weight = 0.2
```

## Section Reference

### `[index]`

Controls which files are indexed.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `include` | string[] | See above | Glob patterns for files to include |
| `exclude` | string[] | See above | Additional exclusion patterns (on top of `.gitignore`) |
| `use_gitignore` | bool | `true` | Whether to respect `.gitignore` files |

### `[embedding]`

Controls embedding model and batch processing.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `model` | string | `"all-MiniLM-L6-v2"` | ONNX embedding model name. Downloaded to `~/.cache/bobbin/models/` on first use. |
| `batch_size` | int | `32` | Number of chunks to embed per batch |

### `[embedding.context]`

Controls contextual embedding, where chunks are embedded with surrounding source lines for better retrieval.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `context_lines` | int | `5` | Lines of context before and after each chunk |
| `enabled_languages` | string[] | `["markdown"]` | Languages where contextual embedding is active |

### `[search]`

Controls search behavior defaults.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `default_limit` | int | `10` | Default number of results returned |
| `semantic_weight` | float | `0.9` | Balance between semantic (1.0) and keyword (0.0) in hybrid mode |

### `[hooks]`

Controls Claude Code hook behavior for automatic context injection.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `threshold` | float | `0.5` | Per-result filter on normalized RRF scores |
| `budget` | int | `300` | Maximum lines of injected context |
| `content_mode` | string | `"full"` | Content display mode: `full`, `preview`, or `none` |
| `min_prompt_length` | int | `20` | Skip injection for prompts shorter than this |
| `gate_threshold` | float | `0.45` | Minimum raw semantic similarity (cosine, before RRF) to inject at all. If the top result's score is below this, the entire injection is skipped. |
| `dedup_enabled` | bool | `true` | Skip injection when search results match the previous prompt's session ID (same top-10 chunks = same session) |

### `[git]`

Controls temporal coupling analysis from git history.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `coupling_enabled` | bool | `true` | Enable temporal coupling analysis |
| `coupling_depth` | int | `5000` | How many commits back to analyze |
| `coupling_threshold` | int | `3` | Minimum co-changes to establish a coupling relationship |

### `[context]`

Tunes context assembly — the bridging + knowledge-expansion pipeline that is Bobbin's core differentiator. These knobs were previously hardcoded (and inconsistent) at each call site; surfacing them here lets you tune behavior without recompiling. Defaults match the tuned production values used by the hook and HTTP injection paths. CLI/HTTP request parameters still override these per-call.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bridge_boost_factor` | float | `0.3` | Score multiplier for bridge-boosted files: `final_score *= (1.0 + factor)`. Only used in `boost`/`boost_inject` bridge modes. |
| `max_bridged_files` | int | `2` | Max bridged files to fetch chunks for. Prevents bridge explosion from doc→source blame chains. |
| `max_bridged_chunks_per_file` | int | `1` | Max chunks kept per bridged file (first N by start line). |
| `coupling_threshold` | float | `0.1` | Minimum coupling **score** (0.0–1.0) for a coupled file to enter context. Distinct from `[git].coupling_threshold`, which is an integer co-change **count** applied during indexing. |
| `knowledge_budget_pct` | float | `15.0` | Percent of the line budget reserved for knowledge-graph expansion (requires the `knowledge` feature). |
| `knowledge_max_hops` | int | `2` | Max graph-traversal hops for knowledge expansion. |

### `[feedback]`

Tunes cross-agent feedback boosting. Files rated "useful" by other agents for similar queries get a bounded score boost during context assembly.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `boost_max` | float | `0.3` | Maximum feedback boost multiplier. Actual boost is `min(score * boost_weight, boost_max)`. |
| `boost_weight` | float | `0.2` | Weight multiplier applied to raw feedback scores. |
