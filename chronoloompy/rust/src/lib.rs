//! PyO3 bindings for the [`chronoloom`] crate.
//!
//! This crate is an implementation detail of the `chronoloompy` Python
//! package rather than a library in its own right: its only export is a
//! `#[pymodule]`. Rust consumers should depend on [`chronoloom`] directly.
//!
//! Everything here should stay a thin adapter — real logic belongs in
//! [`chronoloom`], so that Rust and Python share one implementation.

use pyo3::prelude::*;

mod algebra;

/// Rust extension module, imported from Python as `chronoloompy._core`.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(algebra::intersection, m)?)?;
    Ok(())
}
