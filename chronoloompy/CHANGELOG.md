# Unreleased

## Added

- Test coverage for the Rust binding via `chronoloompy._core.intersection`,
  replacing the incidental coverage the removed demo functions provided.
- Pre-commit hooks: formatting on commit, formatting + lint + type-check + tests
  (Python and Rust) on push.
- Rust unit tests and a `cargo clippy`/`cargo test` CI job.
- mypy strict type checking.
- Release automation: a `chronoloompy-v*` tag builds the wheel matrix and
  publishes to PyPI via Trusted Publishing (OIDC, no stored token) after a manual
  approval, attaching a build-provenance attestation and cutting a GitHub Release
  from this changelog. The tag is rejected unless its commit is on `main`, its
  version matches `pyproject.toml`, and this file documents it.

## Fixed

- The Rust core is now the standalone [`chronoloom`](../chronoloom) crate,
  consumed through a thin pyo3 binding in `rust/`. Algebra implemented there is
  shared with Rust consumers instead of being locked inside the binding layer.
- `pyproject.toml` metadata consolidated into a single PEP 621 `[project]` table;
  runtime dependencies now actually ship in built wheels.
- The make targets that build the binding now pin `PYO3_PYTHON` to this
  package's own `.venv`. pyo3 otherwise resolves its interpreter from
  `$VIRTUAL_ENV`, so an unrelated -- or stale, after a directory rename --
  activated venv would break `cargo clippy`/`cargo test` and the pre-push hooks.

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

# 0.0.1

- Time point and interval models

# 0.0.0

- Sets up the project.
