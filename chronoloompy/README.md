# chronoloompy

Temporal data processing primitives for Python, backed by the
[`chronoloom`](../chronoloom) Rust crate.

```sh
pip install chronoloompy
```

```python
import numpy as np
from chronoloompy import Batch, SequenceSystem, TimeIntervalSequence

machine = SequenceSystem()
temperature = machine.add_points(
    "temperature", np.arange(0, 600, 10), np.full(60, 21.5)
)
machine.add_points("log", [0], ["boot"])
up = machine.add_spans("up", [0, 300], [200, 600])

machine.record_batch(
    Batch().points("temperature", [600], [22.0]).spans("up", [600], [660])
)

temperature.range(100, 200)  # (timestamps, values), as NumPy arrays
idle = up - TimeIntervalSequence("busy", [50], [80])
idle.label  # 'up \\ busy'
idle.spans  # array([[  0,  50], [ 80, 200], [300, 660]])
```

Intervals are half-open — `start` is included, `end` is excluded — and a
timeline keeps its spans merged, so touching spans become one.

## Memory model

`chronoloompy` is only an API: every class is implemented in Rust, and the data
never moves into Python.

- **Sequences live in Rust.** `machine["temperature"]` is a handle onto the
  system's member, not a copy. `SequenceSystem.insert(sequence)` moves the
  sequence's events in, and `sequence` becomes a handle onto them there.
- **Reads are zero-copy.** `timestamps`, `values`, `spans`, `to_numpy()` and
  `range()` return read-only NumPy arrays over the Rust memory. `str` values are
  the exception: NumPy cannot read them in place, so they are copied.
- **Views pin their sequence.** While any such array is alive, a change that
  could move or free the memory under it — recording into that sequence,
  removing it, replacing it — raises `BufferError`, just as `bytearray` does.
  Delete the arrays, or copy them with `numpy.array(view)`, to change it again.
  Other members of the same system stay writable.
- **Writes copy once.** Building a sequence from NumPy arrays reads them in
  place and writes the events straight into the core's own layout; sorting and
  merging then run with the GIL released.

Point sequences carry `float64`, `int64`, `bool` or `str` values, inferred from
the values given or chosen with `dtype=`.

## Relationship to `chronoloom`

`chronoloom` is a standalone Rust crate published to
[crates.io](https://crates.io/crates/chronoloom) and usable from any Rust
project. `chronoloompy` exposes it through a thin pyo3 binding
([`rust/`](rust)): the algebra is implemented once, in `chronoloom`, and the
binding only converts values and guards the memory NumPy reads.

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

The extension rebuilds automatically when `rust/**/*.rs`, `rust/Cargo.toml`, or
`rust/Cargo.lock` change — see `cache-keys` in `pyproject.toml`. Editing
`../chronoloom` does **not** rebuild it: the binding depends on the released
crate from crates.io, so a local core change only reaches Python once it is
published and `rust/Cargo.toml` requires that version.

Run `make setup` from the repository root once to install the git hooks.

## License

MIT — see [LICENSE](LICENSE).
