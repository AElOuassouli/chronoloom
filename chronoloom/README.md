# chronoloom

Set algebra over time points and intervals — a small, dependency-free Rust
library.

Intervals are `(start, end)` pairs of `i64` timestamps and are **half-open**:
`start` is included, `end` is excluded. Two intervals that merely touch
(`[0, 5)` and `[5, 9)`) therefore do not overlap.

```toml
[dependencies]
chronoloom = "0.1"
```

```rust
use chronoloom::algebra::intersection;

assert_eq!(intersection((0, 5), (3, 9)), Some((3, 5)));
assert_eq!(intersection((0, 2), (5, 9)), None);
```

## Python

Python bindings are published separately as
[`chronoloompy`](https://pypi.org/project/chronoloompy/), which wraps this
crate and adds a pydantic-based model layer. It lives in
[`../chronoloompy`](../chronoloompy) in this repository.

## Development

```sh
make lint   # cargo fmt --check, clippy, and the "no pyo3" assertion
make test   # unit and doc tests
```

This crate must never depend on pyo3 — that is what makes it usable from any
Rust project. `make lint` enforces it.

## License

MIT — see [LICENSE](LICENSE).
