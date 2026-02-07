# Task: Add model management CLI commands

## Summary

Add `bobbin models` subcommand for listing available models, showing current model info, and validating custom models.

## Files

- `src/cli/models.rs` (new)
- `src/cli/mod.rs` (modify)

## Commands

### `bobbin models list`

Show available built-in models with their properties:

```
Built-in embedding models:

  all-MiniLM-L6-v2 *     384 dim, 256 max tokens  (fast, general purpose)
  all-MiniLM-L12-v2      384 dim, 256 max tokens  (better quality)
  bge-small-en-v1.5       384 dim, 512 max tokens  (good for code)
  bge-base-en-v1.5        768 dim, 512 max tokens  (high quality)
  gte-small               384 dim, 512 max tokens  (balanced)
  gte-base                768 dim, 512 max tokens  (high quality)
  nomic-embed-text-v1.5   768 dim, 8192 max tokens (long context)

  * = currently configured

Custom models: set embedding.model to a local path containing model.onnx and tokenizer.json
```

### `bobbin models info`

Show details about the currently configured model and index state:

```
Current model: all-MiniLM-L6-v2
  Dimension: 384
  Max sequence length: 256
  Cache path: ~/.cache/bobbin/bobbin/models/all-MiniLM-L6-v2/

Index state:
  Indexed with: all-MiniLM-L6-v2 (384 dim)
  Status: ✓ Model matches index
```

Or if there's a mismatch:
```
  Indexed with: all-MiniLM-L6-v2 (384 dim)
  Configured: bge-base-en-v1.5 (768 dim)
  Status: ⚠ Model mismatch - run `bobbin index --force` to re-index
```

### `bobbin models validate <path>`

For custom models - validate that a local model directory has the right files and works:

```
Validating model at /path/to/model...
  ✓ model.onnx found
  ✓ tokenizer.json found
  ✓ Model loads successfully
  ✓ Output dimension: 768
  ✓ Compatible with bobbin
```

## Dependencies

- Requires Task 1 (dynamic dimensions) and Task 2 (expanded registry)

## Acceptance Criteria

- [ ] `bobbin models list` shows all available models
- [ ] `bobbin models info` shows current config vs index state
- [ ] `bobbin models validate` checks custom model compatibility
- [ ] JSON output supported on all subcommands
