"""Type stubs for the Rust extension module `chronoloompy._core`.

Implemented in ../../rust, which is a thin pyo3 adapter over the `chronoloom`
crate. Keep these signatures in sync with the `#[pyfunction]`s there.

The module currently exports nothing: `chronoloom` moved its interval
operations onto `TimeIntervalEvent`, and the binding cannot reach them until
rust/Cargo.toml requires a published release of the crate that carries them.
"""
