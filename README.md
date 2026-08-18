# chronoloom

Temporal data processing — sequences, intervals, and the algebra over them —
shipped as two independently versioned and published libraries.

| Library | Language | Registry | Source |
| --- | --- | --- | --- |
| `chronoloom` | Rust | [crates.io](https://crates.io/crates/chronoloom) | [`chronoloom/`](chronoloom) |
| `chronoloompy` | Python | [PyPI](https://pypi.org/project/chronoloompy/) | [`chronoloompy/`](chronoloompy) |

[`chronoloom`](chronoloom) holds the algebra and has **no pyo3 dependency**, so
any Rust project can depend on it. [`chronoloompy`](chronoloompy) wraps it
through a thin pyo3 binding ([`chronoloompy/rust`](chronoloompy/rust)) and adds
a Python-native layer — pydantic models today, more later.

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

Each library also has its own makefile with the same targets, if you want to
work on just one:

```sh
make -C chronoloom test
make -C chronoloompy test
```

Git hooks (installed by `make setup`) run formatting on every commit, and
formatting, linting, type-checking, and both test suites on every push.

## Releasing

The two libraries release independently, distinguished by tag prefix:

| Tag | Effect |
| --- | --- |
| `chronoloom-v0.1.0` | publishes the Rust crate to crates.io |
| `chronoloompy-v0.1.0` | builds wheels and publishes to PyPI |

An unprefixed `v*` tag intentionally does nothing.

PyPI uses trusted publishing (no secret needed). **crates.io requires a
`CARGO_REGISTRY_TOKEN` repository secret** — add it under Settings → Secrets and
variables → Actions before the first `chronoloom-v*` tag. Every change to the
core also runs `cargo publish --dry-run` in CI, since published crate versions
are permanent and cannot be reused.

## License

MIT — see [LICENSE](LICENSE).
