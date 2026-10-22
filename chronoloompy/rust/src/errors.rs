//! The core's errors as Python exceptions, and the failures only the binding
//! can have.

use pyo3::exceptions::{PyBufferError, PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;

pyo3::create_exception!(
    _core,
    IntervalError,
    PyValueError,
    "A span whose end is not strictly after its start, or a bound shifted out of range."
);

pyo3::create_exception!(
    _core,
    RecordError,
    PyValueError,
    "Events that could not be recorded into a system. Nothing was recorded."
);

/// Convert a core interval failure into its Python exception.
pub fn interval_error(error: chronoloom::IntervalError) -> PyErr {
    IntervalError::new_err(error.to_string())
}

/// Convert a core recording failure into its Python exception.
pub fn record_error(error: chronoloom::RecordError) -> PyErr {
    RecordError::new_err(error.to_string())
}

/// The error for a change the core allows only outside a system.
pub fn belongs_to_system(label: &str) -> PyErr {
    PyTypeError::new_err(format!(
        "`{label}` belongs to a SequenceSystem; take it out with SequenceSystem.remove first"
    ))
}

/// The error for a handle whose system no longer holds a sequence of its
/// shape for its attribute.
pub fn missing_member(attribute: &str) -> PyErr {
    PyKeyError::new_err(format!(
        "the system no longer holds a sequence of this shape for `{attribute}`"
    ))
}

/// The error for a change that would move or free memory a NumPy view reads.
pub fn exported(label: &str) -> PyErr {
    PyBufferError::new_err(format!(
        "`{label}` cannot be changed while NumPy views of it exist; delete them first"
    ))
}
