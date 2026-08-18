use pyo3::prelude::*;


[pyfunction]
fn intersection(sequence_a: Vec<)




#[pymodule]
fn algebra(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_function(wrap_pyfunction!(sum_as_int, m)?)?;
    Ok(())
}