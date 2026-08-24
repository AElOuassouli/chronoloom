# Unreleased

## Added

- `TimeIntervalEvent` now validates its bounds: `end_timestamp` must be
  strictly after `start_timestamp`, so empty and inverted spans are rejected
  instead of being silently accepted.
- Tests for both primitive models, and a smoke test proving the compiled
  `_core` extension imports.

## Removed

- `chronoloompy._core.intersection`, and the pyo3 adapter behind it. The
  `chronoloom` crate moved interval operations off its free-function `algebra`
  module and onto `TimeIntervalEvent`; `_core` exports nothing until the
  binding's `chronoloom` requirement names a release carrying that API.
- `chronoloompy.algebra`, including the unimplemented `interval_union` and
  `interval_intersection` stubs. Interval algebra is implemented once in
  `chronoloom` and will reach Python through the binding, not as a second
  Python implementation.
- `TimeIntervalEvent.left_open` and `TimeIntervalEvent.right_open`. The core is
  half-open only, so configurable endpoints described a model the algebra never
  had.
- The `attribute` field on both primitives, folded into `value`. Two payload
  fields with overlapping meaning invited events that carried their data in
  whichever one the caller happened to pick.

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
