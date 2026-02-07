# Task: Expand model registry and support custom models

## Summary

Open up the model registry from a hardcoded 3-model switch statement to a flexible system supporting both built-in models and user-provided ONNX models.

## Files

- `src/index/embedder.rs` (modify) - expand registry, add custom model support
- `src/config.rs` (modify) - add model config options

## Changes

### Expanded built-in registry

Add more models to `ModelConfig::get()`:

| Model | Dimension | Max Seq | Use case |
|-------|-----------|---------|----------|
| `all-MiniLM-L6-v2` | 384 | 256 | Fast, general (current default) |
| `bge-small-en-v1.5` | 384 | 512 | Already registered |
| `gte-small` | 384 | 512 | Already registered |
| `nomic-embed-text-v1.5` | 768 | 8192 | High quality, long context |
| `all-MiniLM-L12-v2` | 384 | 256 | Better quality than L6 |
| `bge-base-en-v1.5` | 768 | 512 | High quality |
| `gte-base` | 768 | 512 | High quality |

### Custom model support

When `ModelConfig::get()` doesn't match a built-in name, check for a custom model:

```rust
impl ModelConfig {
    pub fn get(name: &str) -> Result<Self> {
        // 1. Check built-in registry
        if let Some(config) = BUILTIN_MODELS.get(name) {
            return Ok(config.clone());
        }

        // 2. Check for custom model at path
        // If name looks like a path or contains '/', treat as local model
        if name.contains('/') || name.contains('.') {
            return Self::from_local(name);
        }

        // 3. Try HuggingFace model ID
        // Format: "org/model-name" or just "model-name"
        bail!("Unknown model: {}. Use a built-in model name or path to local ONNX model.", name);
    }

    fn from_local(path: &str) -> Result<Self> {
        // Look for model.onnx and tokenizer.json at path
        // Auto-detect dimension by loading model and checking output shape
        // Or read from a metadata.json if present
    }
}
```

### Config changes (`.bobbin/config.toml`)

Extend the `[embedding]` section to support full model configuration:

```toml
[embedding]
model = "all-MiniLM-L6-v2"    # Built-in name, HuggingFace ID, or local path
batch_size = 32                 # existing

# Optional overrides (auto-detected from model if omitted):
# dimension = 384              # embedding dimension
# max_sequence_length = 256    # max input tokens

# For custom local models:
# model = "/path/to/my-model"
# dimension = 768              # required for custom models if auto-detect fails

# For HuggingFace models not in the built-in registry:
# model = "BAAI/bge-large-en-v1.5"
# dimension = 1024
```

The config should support three model sources:
1. **Built-in name** (e.g., `"all-MiniLM-L6-v2"`) - auto-downloads, all properties known
2. **Local path** (e.g., `"/home/user/models/custom"`) - must contain `onnx/model.onnx` + `tokenizer.json`
3. **HuggingFace ID** (e.g., `"BAAI/bge-large-en-v1.5"`) - auto-downloads from HF, dimension auto-detected or specified

Update `EmbeddingConfig` in `src/config.rs`:
```rust
pub struct EmbeddingConfig {
    pub model: String,
    pub batch_size: usize,
    pub context: ContextualEmbeddingConfig,
    pub dimension: Option<usize>,          // NEW: optional override
    pub max_sequence_length: Option<usize>, // NEW: optional override
}
```

### Model download for new built-ins

Update `ensure_model()` to handle all built-in models. The HuggingFace download pattern using the Xenova organization should work for all sentence-transformer models.

### Auto-detection of dimension

When loading a model for the first time:
1. Load the ONNX session
2. Run a test embedding of a known string
3. Check the output dimension
4. Store as the model's dimension

This avoids hardcoding dimensions per model and works for custom models.

## Tests

- Test loading each built-in model config
- Test custom model path resolution
- Test dimension auto-detection
- Test unknown model name gives clear error

## Acceptance Criteria

- [ ] 7+ built-in models available
- [ ] Custom local model paths work
- [ ] Dimensions auto-detected for new models
- [ ] Clear error messages for invalid model names
- [ ] `bobbin index` with a new model triggers full re-index with correct dimension
