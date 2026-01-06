# Tambour Development Guide

## Testing

Tambour uses pytest for testing. Tests are located in the `tests/` directory.

### Running Tests

**Option 1: Using a virtual environment (recommended)**

```bash
cd tambour

# Create and activate a virtual environment
python3 -m venv .venv
source .venv/bin/activate

# Install tambour with dev dependencies
pip install -e ".[dev]"

# Run tests
python -m pytest tests/ -v
```

**Option 2: Quick test run (if venv already exists)**

```bash
cd tambour
source .venv/bin/activate
python -m pytest tests/ -v
```

### Test Structure

- `tests/test_config.py` - Configuration parsing tests
- `tests/test_events.py` - Event dispatching and event type tests
- `tests/test_context.py` - Context provider tests
- `tests/test_heartbeat.py` - Heartbeat writer tests

### Writing Tests

Follow existing patterns in the test files. Key conventions:

1. Use pytest fixtures for shared setup
2. Group related tests in classes (e.g., `TestPluginConfig`, `TestToolEvent`)
3. Use descriptive test names that explain what's being tested
4. Test both success and error cases

### Troubleshooting

**"externally-managed-environment" error:**
Python 3.12+ on macOS requires a virtual environment. Use Option 1 above.

**Module not found errors:**
Ensure you've installed the package in editable mode with `pip install -e ".[dev]"`.
