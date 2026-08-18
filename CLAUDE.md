# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Layout

Two independently versioned and published libraries:

- `chronoloom/` — standalone Rust library, published to crates.io. It must
  never depend on pyo3; `make lint` asserts this, because being usable without
  pyo3 is the entire point of the crate.
- `chronoloompy/` — Python library, published to PyPI. Its pyo3 binding lives
  in `chronoloompy/rust/` and depends on the **released `chronoloom` crate from
  crates.io**, not on the sibling directory.

New algebra belongs in `chronoloom` so Rust and Python share one
implementation. `chronoloompy/rust` should stay thin adapters over it, not a
second home for logic.

Because the dependency is a registry version rather than a path, exposing new
core algebra to Python is a two-step release: land and release the core
(`chronoloom-v*`), then bump the `chronoloom` requirement in
`chronoloompy/rust/Cargo.toml` and release the Python side. An edit to
`chronoloom/` does **not** reach `chronoloompy` until you do — `make test` at
the root will happily pass with the two out of sync, because each library is
tested against what it actually builds against.

## Changelog

Each library has its own `CHANGELOG.md`. Always update the one belonging to the
library you changed (both, if the change spans both) under the `Unreleased`
section, using `Added` / `Fixed` / `Removed` subsections (only include the
subsections that apply). Add a new `Unreleased` section at the top of the file
if one is not already present.

## Git

Never run `git commit` or `git push`. The user runs these themselves.

## Releasing

Release tags are prefixed per library and each drives its own workflow:
`chronoloom-v*` publishes the crate to crates.io via
`.github/workflows/cd-chronoloom.yml`, `chronoloompy-v*` builds wheels and
publishes to PyPI via `.github/workflows/cd-chronoloompy.yml`. An unprefixed `v*`
tag does nothing. Both publish with Trusted Publishing (OIDC) — there are no
registry tokens in repository secrets — and both pause for a manual approval in
the `crates-io` / `pypi` GitHub environments.

To cut a release, from a clean checkout of `main`, run one of:

```sh
make chronoloom-release-fix      # or -minor / -major
make chronoloompy-release-fix    # or -minor / -major
```

(equivalently `make -C <library> release-fix` from inside a library). Each bumps
the version in that library's manifest, dates its changelog's `# Unreleased`
heading, refreshes the lockfile — both `uv.lock` and `chronoloom/Cargo.lock`
embed the version, and CI runs with `UV_LOCKED=1` — then commits and creates an
annotated tag. There is deliberately no combined target: the libraries version
independently and a tag releases exactly one of them.

Nothing is pushed; the command prints the `git push` to run, and the command to
undo. Releases must be cut from `main` with a clean tree and a non-empty
`# Unreleased` section — the script refuses otherwise, before touching any file.

Before publishing, the workflow refuses the release if the tagged commit is not
on `main`, if the tag does not match the manifest version, or if the changelog
has no section for that version. That section also becomes the GitHub Release
notes, so keep the `# <version>` / `## Added|Fixed|Removed` shape intact —
`.github/scripts/changelog.py` parses it.

## Before finishing a task or plan

Always run `make lint` and `make test` from the repository root as the final
step — they fan out to both libraries — and fix anything they surface. The
per-library equivalents are `make -C <library> lint` and
`make -C <library> test`, or the individual commands (`ruff check`,
`ruff format --check`, `mypy`, `cargo fmt --check`, `cargo clippy`, `pytest`,
`cargo test`). Do not report a task as complete while any of these are failing.
