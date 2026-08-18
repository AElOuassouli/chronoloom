# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Layout

Two independently versioned and published libraries:

- `chronoloom/` — standalone Rust library, published to crates.io. It must
  never depend on pyo3; `make lint` asserts this, because being usable without
  pyo3 is the entire point of the crate.
- `chronoloompy/` — Python library, published to PyPI. Its pyo3 binding lives
  in `chronoloompy/rust/` and path-depends on `chronoloom`.

New algebra belongs in `chronoloom` so Rust and Python share one
implementation. `chronoloompy/rust` should stay thin adapters over it, not a
second home for logic.

## Changelog

Each library has its own `CHANGELOG.md`. Always update the one belonging to the
library you changed (both, if the change spans both) under the `Unreleased`
section, using `Added` / `Fixed` / `Removed` subsections (only include the
subsections that apply). Add a new `Unreleased` section at the top of the file
if one is not already present.

## Git

Never run `git commit` or `git push`. The user runs these themselves.

## Releasing

Release tags are prefixed per library: `chronoloom-v*` publishes the crate to
crates.io, `chronoloompy-v*` builds wheels and publishes to PyPI. An unprefixed
`v*` tag does nothing.

## Before finishing a task or plan

Always run `make lint` and `make test` from the repository root as the final
step — they fan out to both libraries — and fix anything they surface. The
per-library equivalents are `make -C <library> lint` and
`make -C <library> test`, or the individual commands (`ruff check`,
`ruff format --check`, `mypy`, `cargo fmt --check`, `cargo clippy`, `pytest`,
`cargo test`). Do not report a task as complete while any of these are failing.
