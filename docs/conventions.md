# Repository conventions

How the repository is organised, so that each library can be found, changed,
documented and released the same way, by people and by coding agents.

## Layers of documentation

| Layer | Where | Holds |
| --- | --- | --- |
| Overview | [README.md](../README.md) | What the repository is, its two libraries, how to start |
| Repository procedure | [docs/](.) | These conventions, [releasing](releasing.md) |
| Library | `<library>/README.md`, `<library>/CHANGELOG.md` | What the library does, its invariants, its own changes |
| Agent configuration (private) | `CLAUDE.md`, `.claude/`, `<library>/CLAUDE.md` | Workflow rules, per-technology conventions |

A fact lives in one place; the others link to it.

The library READMEs are also the pages published on
[crates.io](https://crates.io/crates/chronoloom) and
[PyPI](https://pypi.org/project/chronoloompy/), so they are written for users of
the library. Repository procedure goes in `docs/`, not in them.

Agent configuration is gitignored: it may point to these documents, but nothing
tracked depends on it.

## Libraries

Two libraries, versioned and published independently:

- [`chronoloom/`](../chronoloom): the Rust library, published to crates.io. It
  never depends on pyo3, because being usable without pyo3 is the point of the
  crate. `make lint` asserts it.
- [`chronoloompy/`](../chronoloompy): the Python library, published to PyPI. Its
  pyo3 binding lives in [`chronoloompy/rust/`](../chronoloompy/rust) and depends
  on the **released** `chronoloom` crate from crates.io, not on the sibling
  directory.

New algebra belongs in `chronoloom`, so that Rust and Python share one
implementation. `chronoloompy/rust` stays a thin adapter over it, not a second
home for logic.

Because the binding depends on a registry version, a change to `chronoloom/`
reaches `chronoloompy` only after a release; see
[releasing](releasing.md#exposing-a-core-change-to-python).

## Changelogs

Each library has its own `CHANGELOG.md`. A change goes in the changelog of the
library it touches (both, if it spans both), under a `# Unreleased` heading at
the top of the file, in `## Added`, `## Fixed` or `## Removed` subsections
(only those that apply):

```markdown
# Unreleased

## Added

- ...
```

Keep that shape intact: [`.github/scripts/changelog.py`](../.github/scripts/changelog.py)
parses it, the release refuses a version with no section, and the section
becomes the GitHub Release notes.

## Makefiles

The root [makefile](../makefile) fans out to both libraries:

| Target | Does |
| --- | --- |
| `make setup` | Builds the Python environment and installs the git hooks |
| `make fmt` | Formats both libraries |
| `make lint` | Lints and type-checks both libraries |
| `make test` | Tests both libraries |
| `make clean` | Removes build artifacts and caches |
| `make fmt-rust`, `lint-rust`, `test-rust` | Rust-only slices, used by the git hooks |
| `make <library>-release-fix\|minor\|major` | Cuts a release; see [releasing](releasing.md) |

Each library has its own makefile with the same verbs: `make -C chronoloom test`,
`make -C chronoloompy lint`, and so on, plus `check`, which runs `fmt`, `lint` and `test` in turn. `make help`
lists them.

## Git hooks

Installed by `make setup` from [`.pre-commit-config.yaml`](../.pre-commit-config.yaml).
On every commit they format (ruff, cargo fmt) and run the file hygiene checks;
on every push they also run clippy, mypy and both test suites.
