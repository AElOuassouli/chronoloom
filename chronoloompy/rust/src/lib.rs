//! PyO3 bindings for the [`chronoloom`] crate.
//!
//! This crate is an implementation detail of the `chronoloompy` Python
//! package rather than a library in its own right: its only export is a
//! `#[pymodule]`. Rust consumers should depend on [`chronoloom`] directly.
//!
//! Everything here should stay a thin adapter — real logic belongs in
//! [`chronoloom`], so that Rust and Python share one implementation.

use pyo3::prelude::*;

/// Rust extension module, imported from Python as `chronoloompy._core`.
///
/// Currently exports nothing. `chronoloom` moved its interval operations off
/// the free-function `algebra` module and onto `TimeIntervalEvent`, and this
/// binding compiles against the *published* crate — see the `chronoloom`
/// requirement in `Cargo.toml`. Adapters return once that requirement names a
/// release carrying the new API.
#[pymodule]
fn _core(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
