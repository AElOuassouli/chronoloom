//! Native acceleration kernels for the `timewarp` package.

use pyo3::prelude::*;

mod algebra;

/// Format the sum of two numbers as a string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> String {
    (a + b).to_string()
}

/// Return the sum of two numbers.
#[pyfunction]
fn sum_as_int(a: usize, b: usize) -> usize {
    a + b
}

/// Rust extension module, imported from Python as `timewarp._timewarp`.
#[pymodule]
fn _timewarp(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_function(wrap_pyfunction!(sum_as_int, m)?)?;
    m.add_function(wrap_pyfunction!(algebra::intersection, m)?)?;
    Ok(())
}
