//! Adapters exposing `chronoloom::algebra` to Python.

use pyo3::prelude::*;

/// Intersect two half-open intervals `[start, end)`.
///
/// Returns `None` when the intervals do not overlap.
#[pyfunction]
pub fn intersection(a: (i64, i64), b: (i64, i64)) -> Option<(i64, i64)> {
    chronoloom::algebra::intersection(a, b)
}
