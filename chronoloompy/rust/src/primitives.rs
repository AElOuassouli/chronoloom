//! The two event shapes, carrying any Python object as their value.

use pyo3::prelude::*;
use pyo3::types::PyList;

use chronoloom::Timestamp;

use crate::errors::interval_error;

/// A value observed at a single instant, with no duration.
#[pyclass(frozen, module = "chronoloompy", name = "TimePointEvent")]
pub struct PyTimePointEvent {
    pub inner: chronoloom::TimePointEvent<Py<PyAny>>,
}

#[pymethods]
impl PyTimePointEvent {
    #[new]
    fn new(timestamp: Timestamp, value: Py<PyAny>) -> Self {
        Self {
            inner: chronoloom::TimePointEvent::new(timestamp, value),
        }
    }

    #[getter]
    fn timestamp(&self) -> Timestamp {
        self.inner.timestamp()
    }

    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.inner.value().clone_ref(py)
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, Self>) -> PyResult<bool> {
        let other = &other.get().inner;
        if self.inner.timestamp() != other.timestamp() {
            return Ok(false);
        }

        self.inner.value().bind(py).eq(other.value())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let value = self.inner.value().bind(py).repr()?;
        Ok(format!(
            "TimePointEvent(timestamp={}, value={value})",
            self.inner.timestamp()
        ))
    }
}

/// A value attached to the half-open span `[start, end)`.
#[pyclass(frozen, module = "chronoloompy", name = "TimeIntervalEvent")]
pub struct PyTimeIntervalEvent {
    pub inner: chronoloom::TimeIntervalEvent<Py<PyAny>>,
}

impl PyTimeIntervalEvent {
    /// A valueless span, as every span inside a timeline is.
    pub fn from_span(py: Python<'_>, span: &chronoloom::TimeIntervalEvent<()>) -> Self {
        Self {
            inner: chronoloom::TimeIntervalEvent::new(span.start(), span.end(), py.None())
                .expect("a span read from the core is already valid"),
        }
    }
}

#[pymethods]
impl PyTimeIntervalEvent {
    #[new]
    #[pyo3(signature = (start, end, value = None))]
    fn new(
        py: Python<'_>,
        start: Timestamp,
        end: Timestamp,
        value: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let value = value.unwrap_or_else(|| py.None());
        let inner =
            chronoloom::TimeIntervalEvent::new(start, end, value).map_err(interval_error)?;

        Ok(Self { inner })
    }

    #[getter]
    fn start(&self) -> Timestamp {
        self.inner.start()
    }

    #[getter]
    fn end(&self) -> Timestamp {
        self.inner.end()
    }

    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.inner.value().clone_ref(py)
    }

    fn bounds(&self) -> (Timestamp, Timestamp) {
        self.inner.bounds()
    }

    fn duration(&self) -> i64 {
        self.inner.duration()
    }

    fn intersection(&self, py: Python<'_>, other: &Bound<'_, Self>) -> Option<Self> {
        self.inner
            .intersection(&other.get().inner)
            .map(|span| Self::from_span(py, &span))
    }

    fn merged(&self, py: Python<'_>, other: &Bound<'_, Self>) -> Option<Self> {
        self.inner
            .merged(&other.get().inner)
            .map(|span| Self::from_span(py, &span))
    }

    fn union<'py>(
        &self,
        py: Python<'py>,
        other: &Bound<'py, Self>,
    ) -> PyResult<Bound<'py, PyList>> {
        let spans = self.inner.union(&other.get().inner);
        PyList::new(py, spans.iter().map(|span| Self::from_span(py, span)))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, Self>) -> PyResult<bool> {
        let other = &other.get().inner;
        if self.inner.bounds() != other.bounds() {
            return Ok(false);
        }

        self.inner.value().bind(py).eq(other.value())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let value = self.inner.value().bind(py).repr()?;
        Ok(format!(
            "TimeIntervalEvent(start={}, end={}, value={value})",
            self.inner.start(),
            self.inner.end()
        ))
    }
}
