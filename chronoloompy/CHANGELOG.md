# Unreleased

## Added

- Test coverage for the Rust binding via `chronoloompy._core.intersection`,
  replacing the incidental coverage the removed demo functions provided.
- Pre-commit hooks: formatting on commit, formatting + lint + type-check + tests
  (Python and Rust) on push.
- Rust unit tests and a `cargo clippy`/`cargo test` CI job.
- mypy strict type checking.

## Fixed

- The Rust core is now the standalone [`chronoloom`](../chronoloom) crate,
  consumed through a thin pyo3 binding in `rust/`. Algebra implemented there is
  shared with Rust consumers instead of being locked inside the binding layer.
- `pyproject.toml` metadata consolidated into a single PEP 621 `[project]` table;
  runtime dependencies now actually ship in built wheels.

## Removed

- Renamed from `timewarp` to `chronoloompy`; the extension module is now
  `chronoloompy._core` (was `timewarp._timewarp`). Both `timewarp` and
  `timewarpy` were already taken on PyPI by unrelated projects.
- `sum_as_string` / `sum_as_int`, which were maturin template demo functions
  rather than API.
- Poetry (migrated to uv; maturin remains the build backend).
- black (replaced by ruff for formatting).
- pyo3 0.19 / GIL-Ref API (upgraded to 0.29 with the `Bound` API and abi3, so one
  wheel now covers all supported Python versions instead of one per version).

# (v.0.0.1)

- Time point and interval models

# version v0.0.0

- Sets up the project.
