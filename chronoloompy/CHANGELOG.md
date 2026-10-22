# Unreleased

## Added

- The `chronoloom` 0.3 API, implemented in Rust and exposed as Python classes:
  `TimePointEvent`, `TimeIntervalEvent`, `TimePointSequence`,
  `TimeIntervalSequence`, `SequenceSystem` and `Batch`, with the `IntervalError`
  and `RecordError` exceptions. The package is now only an API over the core:
  its Python files hold re-exports and type stubs, no logic.
- `TimeIntervalEvent` validates its bounds: `end` must be strictly after
  `start`, so empty and inverted spans raise `IntervalError`.
- Point sequences carry `float64`, `int64`, `bool` or `str` values, inferred
  from the values given or chosen with `dtype=`.
- Data stays in Rust memory. `system["attribute"]` returns a handle onto a
  member rather than a copy, and `SequenceSystem.insert` moves a sequence's
  events into the system, leaving the object a handle onto them there.
- Zero-copy reads: `timestamps`, `values`, `spans`, `to_numpy()` and `range()`
  return read-only NumPy views of the Rust memory (`str` values are copied).
  While a view is alive, changes that could move or free the memory under it
  raise `BufferError`, as `bytearray` does.
- Building from NumPy arrays reads them in place and writes the events directly
  into the core's layout, a single copy; sorting, merging, set operations and
  batch recording run with the GIL released.
- Tests for every class, and for the memory guarantees above.

## Removed

- The pydantic models `TimePointEvent` and `TimeIntervalEvent`, the NumPy-backed
  `TimePointSequence`, and the `chronoloompy.models` package that held them —
  replaced by the Rust classes, importable from `chronoloompy` directly.
- The `pydantic` dependency.
- `chronoloompy._core.intersection`, superseded by
  `TimeIntervalEvent.intersection`.
- `chronoloompy.algebra`, including the unimplemented `interval_union` and
  `interval_intersection` stubs. Interval algebra is implemented once in
  `chronoloom` and reaches Python through the binding.
- `TimeIntervalEvent.left_open` and `TimeIntervalEvent.right_open`. The core is
  half-open only, so configurable endpoints described a model the algebra never
  had.
- The `attribute` field on both event primitives, folded into `value`. A
  sequence, not an event, says what it records.

## Fixed

- `TimePointEvent.timestamp` accepts any integer rather than only positive
  ones, matching the Rust core: the epoch is the caller's to choose, so
  negative timestamps are ordinary.

- Releasing now runs the full CI suite before publishing. `cd-chronoloompy`
  calls `ci-chronoloompy` as a reusable workflow and gates the PyPI publish on
  it; previously a tag published whatever it pointed at without running a single
  test.
- `ci-chronoloompy` no longer runs on pushes to `main`. A pull request check
  already tests the merge result, so the post-merge run duplicated it — and on a
  release push it produced a second, confusingly identical run that could not
  gate anything.
- The release script no longer deletes the blank line after the changelog
  heading it rewrites (`\s*` matched the following newline).

# 0.1.1 - 2026-08-18

## Added

- Test coverage for the Rust binding via `chronoloompy._core.intersection`,
  replacing the incidental coverage the removed demo functions provided.
- Pre-commit hooks: formatting on commit, formatting + lint + type-check + tests
  (Python and Rust) on push.
- Rust unit tests and a `cargo clippy`/`cargo test` CI job.
- mypy strict type checking.
- `make release-fix` / `release-minor` / `release-major` (and
  `make chronoloompy-release-*` from the repo root) bump the version, date the
  changelog, refresh `uv.lock`, then commit and tag. Nothing is pushed.
- Release automation: a `chronoloompy-v*` tag builds the wheel matrix and
  publishes to PyPI via Trusted Publishing (OIDC, no stored token) after a manual
  approval, attaching a build-provenance attestation and cutting a GitHub Release
  from this changelog. The tag is rejected unless its commit is on `main`, its
  version matches `pyproject.toml`, and this file documents it.

## Fixed

- The Rust core is now the standalone [`chronoloom`](../chronoloom) crate,
  consumed through a thin pyo3 binding in `rust/`. Algebra implemented there is
  shared with Rust consumers instead of being locked inside the binding layer.
  The binding depends on `chronoloom = "0.1"` from crates.io rather than on the
  sibling directory, so every wheel contains core code matching a published
  crate version and the sdist no longer vendors an out-of-tree path dependency.
- `pyproject.toml` metadata consolidated into a single PEP 621 `[project]` table;
  runtime dependencies now actually ship in built wheels.
- `maturin sdist` no longer prints a spurious `manifest path does not exist`
  error or a missing-metadata warning. The `sdist` make target now passes
  `--manifest-path` as CI does, and the binding crate declares the package
  metadata `cargo package --list` expects.
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
