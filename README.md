# chronoloom

Temporal data processing — sequences, intervals, and the algebra over them —
shipped as two independently versioned and published libraries.

| Library | Language | Registry | Source |
| --- | --- | --- | --- |
| `chronoloom` | Rust | [crates.io](https://crates.io/crates/chronoloom) | [`chronoloom/`](chronoloom) |
| `chronoloompy` | Python | [PyPI](https://pypi.org/project/chronoloompy/) | [`chronoloompy/`](chronoloompy) |

[`chronoloom`](chronoloom) holds the algebra and has **no pyo3 dependency**, so
any Rust project can depend on it. [`chronoloompy`](chronoloompy) wraps it
through a thin pyo3 binding ([`chronoloompy/rust`](chronoloompy/rust)) and is
only an API over it: data stays in Rust memory, and Python reads it through
zero-copy NumPy views.

New algebra belongs in `chronoloom`, so that Rust and Python share one
implementation rather than diverging.

## Development

Requires [uv](https://docs.astral.sh/uv/) and a Rust toolchain (managed
automatically via `rust-toolchain.toml` if you have [rustup](https://rustup.rs)).

```sh
make setup   # build the env and install git hooks
make fmt     # format both libraries
make lint    # lint and type-check both libraries
make test    # test both libraries
```

Each library also has its own makefile with the same targets
(`make -C chronoloom test`, `make -C chronoloompy test`). Git hooks run
formatting on every commit, and linting, type-checking and both test suites on
every push. See [docs/conventions.md](docs/conventions.md) for how the
repository is organised.

## Releasing

The two libraries release independently: a `chronoloom-v*` tag publishes the
Rust crate to crates.io, a `chronoloompy-v*` tag publishes wheels to PyPI, both
through Trusted Publishing. See [docs/releasing.md](docs/releasing.md).

## License

MIT — see [LICENSE](LICENSE).
