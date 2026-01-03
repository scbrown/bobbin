# Contributing to Tambour

This project uses a mix of Rust (Bobbin) and Python (Tambour).

## Python Development (Tambour)

The `tambour` directory contains the Python-based agent harness.

### Prerequisites

- Python 3.11+
- `pip`

### Setup

1. Navigate to the `tambour` directory:
   ```bash
   cd tambour
   ```

2. Create a virtual environment:
   ```bash
   python3 -m venv .venv
   source .venv/bin/activate
   ```

3. Install dependencies in editable mode:
   ```bash
   pip install -e ".[dev]"
   ```

### Running Tests

We use `pytest` for testing.

1. Ensure your virtual environment is active:
   ```bash
   source .venv/bin/activate
   ```

2. Run the tests:
   ```bash
   pytest
   ```

### Code Style

- Follow PEP 8 guidelines.
- Ensure type hints are used.
