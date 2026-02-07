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
# 0.0 = keyword only, 1.0 = semantic only, default 0.7.
semantic_weight = 0.7

[git]
# Enable temporal coupling analysis (tracks which files
# frequently change together in git history)
coupling_enabled = true

# Number of commits to analyze for coupling relationships
coupling_depth = 1000

# Minimum number of co-changes required to establish a coupling link
coupling_threshold = 3
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
| `semantic_weight` | float | `0.7` | Balance between semantic (1.0) and keyword (0.0) in hybrid mode |

### `[git]`

Controls temporal coupling analysis from git history.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `coupling_enabled` | bool | `true` | Enable temporal coupling analysis |
| `coupling_depth` | int | `1000` | How many commits back to analyze |
| `coupling_threshold` | int | `3` | Minimum co-changes to establish a coupling relationship |
