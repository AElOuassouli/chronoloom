# chronoloom

Set algebra over time points and intervals — a small, dependency-free Rust
library.

Intervals are **half-open**: `start` is included, `end` is excluded. Two
intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore share no instant.

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

## Operations

Two intervals can be intersected — the span where both are active — or united —
the spans covered by either, merged when nothing separates them.

```rust
use chronoloom::TimeIntervalEvent;

let a = TimeIntervalEvent::new(0, 5, "a").unwrap();
let b = TimeIntervalEvent::new(5, 9, "b").unwrap();

// Half-open, so touching intervals share no instant.
assert_eq!(a.intersection(&b), None);

// But together they cover exactly [0, 9), so they merge.
assert_eq!(a.union(&b), vec![TimeIntervalEvent::span(0, 9).unwrap()]);

// A real gap keeps them apart, always ordered by start.
let far = TimeIntervalEvent::new(20, 30, "c").unwrap();
assert_eq!(a.union(&far).len(), 2);
```

Both work on the time dimension only: they accept intervals carrying different
kinds of value, consume neither, and return valueless spans. Reattach a value
with `map` if the result needs to mean something.

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
