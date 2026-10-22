//! PyO3 bindings for the [`chronoloom`] crate.
//!
//! This crate is an implementation detail of the `chronoloompy` Python
//! package rather than a library in its own right: its only export is a
//! `#[pymodule]`. Rust consumers should depend on [`chronoloom`] directly.
//!
//! Everything here is a thin adapter — converting values, choosing among the
//! value types Python can use, and guarding the memory NumPy views read. The
//! algebra itself lives in [`chronoloom`], so Rust and Python share one
//! implementation, and data stays in the core's own structures: Python holds
//! handles onto it rather than copies of it.

use pyo3::prelude::*;

mod errors;
mod exports;
mod primitives;
mod sequence_system;
mod storage;
mod time_interval_sequence;
mod time_point_sequence;
mod values;

/// Rust extension module, imported from Python as `chronoloompy._core`.
#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();

    module.add_class::<primitives::PyTimePointEvent>()?;
    module.add_class::<primitives::PyTimeIntervalEvent>()?;
    module.add_class::<time_point_sequence::PyTimePointSequence>()?;
    module.add_class::<time_interval_sequence::PyTimeIntervalSequence>()?;
    module.add_class::<sequence_system::PySequenceSystem>()?;
    module.add_class::<sequence_system::PyBatch>()?;
    module.add("IntervalError", py.get_type::<errors::IntervalError>())?;
    module.add("RecordError", py.get_type::<errors::RecordError>())?;

    Ok(())
}
