# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Changelog

Always update `CHANGELOG.md` under the `Unreleased` section when making a change,
using `Added` / `Fixed` / `Removed` subsections (only include the subsections that
apply). Add a new `Unreleased` section at the top of the file if one is not
already present.

## Git

Never run `git commit` or `git push`. The user runs these themselves.

## Before finishing a task or plan

Always run `make lint` and `make test` (or the equivalent individual commands:
`ruff check`, `ruff format --check`, `mypy`, `cargo fmt --check`, `cargo clippy`,
`pytest`, `cargo test`) as the final step, and fix anything they surface. Do not
report a task as complete while any of these are failing.
