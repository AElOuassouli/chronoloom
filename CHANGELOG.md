
# Unreleased

## Added

- Pre-commit hooks: formatting on commit, formatting + lint + type-check + tests
  (Python and Rust) on push.
- Rust unit tests and a `cargo clippy`/`cargo test` CI job.
- mypy strict type checking.

## Fixed

- `pyproject.toml` metadata consolidated into a single PEP 621 `[project]` table;
  runtime dependencies now actually ship in built wheels.

## Removed

- Poetry (migrated to uv; maturin remains the build backend).
- black (replaced by ruff for formatting).
- pyo3 0.19 / GIL-Ref API (upgraded to 0.29 with the `Bound` API and abi3, so one
  wheel now covers all supported Python versions instead of one per version).

# (v.0.0.1)

- Time point and interval models

# version v0.0.0

- Sets up the project.
