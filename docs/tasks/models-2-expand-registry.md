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

### Config changes

Add optional model config overrides:
```toml
[embedding]
model = "all-MiniLM-L6-v2"  # or path to local model
# batch_size = 32  # existing
# dimension = 384  # optional override, auto-detected if omitted
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
