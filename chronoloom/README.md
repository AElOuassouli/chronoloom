# chronoloom

Set algebra over time points and intervals — a small, dependency-free Rust
library.

Intervals are **half-open**: `start` is included, `end` is excluded. Two
intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore do not overlap.

```toml
[dependencies]
chronoloom = "0.2"
```

## Primitives

Two event shapes make up the vocabulary, both generic over the value they carry
— a measurement, a label, a set of tags, or nothing at all (`()`):

- `TimePointEvent` anchors a value to a single instant, with no duration.
- `TimeIntervalEvent` attaches a value to a span of time. Construction is
  validated, so `end` is always strictly after `start` and a span is never
  empty.

```rust
use chronoloom::{TimeIntervalEvent, TimePointEvent};

let reading = TimePointEvent::new(1_700_000_000, 21.5_f64);
assert_eq!(reading.timestamp(), 1_700_000_000);

let phase = TimeIntervalEvent::new(0, 60, "warm-up").unwrap();
assert_eq!(phase.duration(), 60);

assert!(TimeIntervalEvent::span(5, 5).is_err());
```

## Algebra

Set operations work on `(start, end)` pairs of `i64` timestamps.

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
