# chronoloompy

Temporal data processing primitives for Python, backed by the
[`chronoloom`](../chronoloom) Rust crate.

```sh
pip install chronoloompy
```

```python
from chronoloompy.models import TimeIntervalEvent

event = TimeIntervalEvent(start_timestamp=0, end_timestamp=5)
```

Intervals are half-open — `start` is included, `end` is excluded.

## Relationship to `chronoloom`

`chronoloom` is a standalone Rust crate published to
[crates.io](https://crates.io/crates/chronoloom) and usable from any Rust
project. `chronoloompy` wraps it through a thin pyo3 binding
([`rust/`](rust)) and adds a Python-native layer on top — pydantic models
today, more later.

Performance-critical algebra should be implemented once in `chronoloom` and
exposed here, rather than reimplemented in Python.

## Development

Requires [uv](https://docs.astral.sh/uv/) and a Rust toolchain (managed
automatically via the repo's `rust-toolchain.toml` if you have
[rustup](https://rustup.rs) installed).

```sh
make install   # uv sync -- builds the extension
make fmt       # ruff format + cargo fmt
make lint      # ruff, mypy, cargo fmt --check, clippy
make test      # pytest + cargo test
make build     # release wheel into dist/
```

The extension rebuilds automatically when `rust/**/*.rs`, `rust/Cargo.toml`,
or the `../chronoloom` sources change — see `cache-keys` in `pyproject.toml`.

Run `make setup` from the repository root once to install the git hooks.

## License

MIT — see [LICENSE](LICENSE).
