# chronoloom

Temporal data processing library — sequences and streams, with performance-critical
operations implemented in Rust and exposed to Python via [pyo3](https://pyo3.rs)/[maturin](https://www.maturin.rs).

## Development setup

Requires [uv](https://docs.astral.sh/uv/) and a Rust toolchain (managed automatically
via `rust-toolchain.toml` if you have [rustup](https://rustup.rs) installed).

```sh
make setup   # uv sync + install git hooks (pre-commit and pre-push)
```

Common tasks:

```sh
make fmt     # format Python (ruff) and Rust (cargo fmt)
make lint    # ruff check, ruff format --check, mypy, cargo fmt --check, cargo clippy
make test    # pytest + cargo test
make build   # build a release wheel into dist/
```

`uv run <cmd>` is the general-purpose inner loop (`uv run pytest`, `uv run python`, ...).
The Rust extension rebuilds automatically when `rust/**/*.rs` or `rust/Cargo.toml` change.

Git hooks (installed by `make setup`) run formatting on every commit, and formatting,
linting, type-checking, and both test suites on every push.
