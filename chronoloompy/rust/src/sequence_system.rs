//! The system of sequences, and batches of events recorded into it.

use std::collections::HashMap;

use pyo3::exceptions::{PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyList;

use chronoloom::{Batch, RecordError, Sequence, SequenceSystem, TimeIntervalEvent, TimePointEvent};
use chronoloom::{TimeIntervalSequence, Timestamp};

use crate::errors::{interval_error, record_error};
use crate::exports::ExportCounter;
use crate::time_interval_sequence::PyTimeIntervalSequence;
use crate::time_point_sequence::PyTimePointSequence;
use crate::values::ValueType;
use crate::values::{events_from, spans_from, with_value_type, PointRef, PointValues, ValueKind};

/// A set of sequences, each corresponding to one attribute.
#[pyclass(module = "chronoloompy", name = "SequenceSystem")]
#[derive(Default)]
pub struct PySequenceSystem {
    pub inner: SequenceSystem,
    /// One counter per attribute ever filed, guarding that member's memory.
    exports: HashMap<String, ExportCounter>,
}

impl PySequenceSystem {
    /// The counter guarding the memory of the member filed under `attribute`.
    pub fn counter(&self, attribute: &str) -> ExportCounter {
        self.exports.get(attribute).cloned().unwrap_or_default()
    }

    /// Refuse if a view of the member filed under `attribute` is alive.
    pub fn ensure_unexported(&self, attribute: &str) -> PyResult<()> {
        self.exports
            .get(attribute)
            .map_or(Ok(()), |counter| counter.ensure_unexported(attribute))
    }

    /// File `sequence` under its attribute, handing back what was filed there.
    fn file(&mut self, sequence: Sequence) -> PyResult<Option<Sequence>> {
        let label = sequence.label();
        self.ensure_unexported(&label)?;
        self.exports.entry(label).or_default();

        Ok(self.inner.insert(sequence))
    }

    /// The value type of the point sequence filed under `attribute`.
    fn point_kind(&self, attribute: &str) -> PyResult<ValueKind> {
        match self.inner.get(attribute) {
            Some(Sequence::TimePoint(points)) => Ok(PointRef::from_any(points).kind()),
            Some(Sequence::TimeInterval(_)) => Err(record_error(RecordError::ShapeMismatch {
                attribute: attribute.to_owned(),
            })),
            None => Err(record_error(RecordError::Unknown {
                attribute: attribute.to_owned(),
            })),
        }
    }

    /// Record one point into the member filed under `attribute`, converting
    /// `value` to that member's value type.
    pub fn record_point_value(
        &mut self,
        attribute: &str,
        timestamp: Timestamp,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let kind = self.point_kind(attribute)?;
        self.ensure_unexported(attribute)?;

        with_value_type!(kind, T => {
            let event = TimePointEvent::new(timestamp, T::value_from(value)?);
            self.inner.record_point(attribute, event).map_err(record_error)
        })
    }

    /// Record a span into the timeline filed under `attribute`.
    pub fn record_span_bounds(
        &mut self,
        attribute: &str,
        start: Timestamp,
        end: Timestamp,
    ) -> PyResult<()> {
        let span = TimeIntervalEvent::span(start, end).map_err(interval_error)?;
        self.ensure_unexported(attribute)?;

        self.inner
            .record_span(attribute, span)
            .map_err(record_error)
    }

    /// Record `batch`, all or nothing, with the GIL released.
    pub fn record(&mut self, py: Python<'_>, batch: Batch, attributes: &[&str]) -> PyResult<()> {
        for attribute in attributes {
            self.ensure_unexported(attribute)?;
        }

        py.detach(|| self.inner.record_batch(batch))
            .map_err(record_error)
    }

    /// Record one attribute's events, all or nothing.
    pub fn record_chunk(&mut self, py: Python<'_>, attribute: &str, chunk: Chunk) -> PyResult<()> {
        let mut batch = Batch::new();
        self.push(&mut batch, attribute, &chunk, py)?;
        self.record(py, batch, &[attribute])
    }

    /// Add `chunk`'s events for `attribute` to `batch`, converted to the
    /// value type of the member they are for.
    fn push(
        &self,
        batch: &mut Batch,
        attribute: &str,
        chunk: &Chunk,
        py: Python<'_>,
    ) -> PyResult<()> {
        match chunk {
            Chunk::Points { timestamps, values } => {
                let kind = self.point_kind(attribute)?;
                with_value_type!(kind, T => {
                    let events = events_from::<T>(timestamps.bind(py), values.bind(py))?;
                    batch.points(attribute, events);
                });
            }
            Chunk::Spans { starts, ends } => {
                if let Some(Sequence::TimePoint(_)) = self.inner.get(attribute) {
                    return Err(record_error(RecordError::ShapeMismatch {
                        attribute: attribute.to_owned(),
                    }));
                }
                batch.spans(attribute, spans_from(starts.bind(py), ends.bind(py))?);
            }
        }

        Ok(())
    }

    /// A handle onto the member filed under `attribute`.
    fn handle(slf: &Bound<'_, Self>, attribute: &str) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        let system = slf.clone().unbind();
        let handle = match slf.borrow().inner.get(attribute) {
            None => return Ok(None),
            Some(Sequence::TimePoint(_)) => Py::new(
                py,
                PyTimePointSequence::member(system, attribute.to_owned()),
            )?
            .into_any(),
            Some(Sequence::TimeInterval(_)) => Py::new(
                py,
                PyTimeIntervalSequence::member(system, attribute.to_owned()),
            )?
            .into_any(),
        };

        Ok(Some(handle))
    }
}

/// A sequence taken out of a system, as a Python object owning it.
fn owned_object(py: Python<'_>, sequence: Sequence) -> PyResult<Py<PyAny>> {
    match sequence {
        Sequence::TimePoint(points) => Ok(Py::new(
            py,
            PyTimePointSequence::owned(PointValues::from_any(points)),
        )?
        .into_any()),
        Sequence::TimeInterval(timeline) => {
            Ok(Py::new(py, PyTimeIntervalSequence::owned(timeline))?.into_any())
        }
    }
}

#[pymethods]
impl PySequenceSystem {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Move `sequence` in under its attribute; it becomes a handle onto the
    /// member. Returns whatever the attribute filed before.
    fn insert(slf: &Bound<'_, Self>, sequence: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        let system = slf.clone().unbind();

        let incoming = if let Ok(points) = sequence.cast::<PyTimePointSequence>() {
            let mut points = points.borrow_mut();
            slf.borrow().ensure_unexported(&points.owned_label()?)?;
            points.move_into(system)?
        } else if let Ok(timeline) = sequence.cast::<PyTimeIntervalSequence>() {
            let mut timeline = timeline.borrow_mut();
            slf.borrow().ensure_unexported(&timeline.owned_label()?)?;
            timeline.move_into(system)?
        } else {
            return Err(PyTypeError::new_err(
                "only a TimePointSequence or a TimeIntervalSequence can join a system",
            ));
        };

        let displaced = slf.borrow_mut().file(incoming)?;
        displaced
            .map(|sequence| owned_object(py, sequence))
            .transpose()
    }

    /// Build a point sequence in place, returning a handle onto it.
    #[pyo3(signature = (attribute, timestamps, values, *, dtype = None))]
    fn add_points(
        slf: &Bound<'_, Self>,
        attribute: &str,
        timestamps: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
        dtype: Option<&str>,
    ) -> PyResult<Py<PyAny>> {
        let sequence = PointValues::from_python(attribute, timestamps, values, dtype)?;
        slf.borrow_mut().file(sequence.into())?;

        Ok(Self::handle(slf, attribute)?.expect("filed just above"))
    }

    /// Build a timeline in place, returning a handle onto it.
    fn add_spans(
        slf: &Bound<'_, Self>,
        attribute: &str,
        starts: &Bound<'_, PyAny>,
        ends: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let spans = spans_from(starts, ends)?;
        let timeline = slf
            .py()
            .detach(|| TimeIntervalSequence::from_spans(attribute, spans));
        slf.borrow_mut().file(timeline.into())?;

        Ok(Self::handle(slf, attribute)?.expect("filed just above"))
    }

    /// Take the sequence for `attribute` out of the system, as an object of
    /// its own, or `None` if there is none.
    fn remove(&mut self, py: Python<'_>, attribute: &str) -> PyResult<Option<Py<PyAny>>> {
        self.ensure_unexported(attribute)?;
        self.inner
            .remove(attribute)
            .map(|sequence| owned_object(py, sequence))
            .transpose()
    }

    fn clear(&mut self) -> PyResult<()> {
        for (attribute, counter) in &self.exports {
            counter.ensure_unexported(attribute)?;
        }

        self.inner.clear();
        Ok(())
    }

    /// A handle onto the sequence for `attribute`, or `None`.
    fn get(slf: &Bound<'_, Self>, attribute: &str) -> PyResult<Option<Py<PyAny>>> {
        Self::handle(slf, attribute)
    }

    fn __getitem__(slf: &Bound<'_, Self>, attribute: &str) -> PyResult<Py<PyAny>> {
        Self::handle(slf, attribute)?.ok_or_else(|| PyKeyError::new_err(attribute.to_owned()))
    }

    fn __contains__(&self, attribute: &str) -> bool {
        self.inner.contains(attribute)
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        PyList::new(py, self.attributes())?
            .as_any()
            .try_iter()
            .map(Bound::into_any)
    }

    /// Every attribute recorded, in order.
    fn attributes(&self) -> Vec<String> {
        self.inner.attributes().cloned().collect()
    }

    /// Every attribute with a handle onto its sequence, in order.
    fn items(slf: &Bound<'_, Self>) -> PyResult<Vec<(String, Py<PyAny>)>> {
        let attributes = slf.borrow().attributes();
        attributes
            .into_iter()
            .map(|attribute| {
                let handle = Self::handle(slf, &attribute)?.expect("listed just above");
                Ok((attribute, handle))
            })
            .collect()
    }

    fn record_point(
        &mut self,
        attribute: &str,
        timestamp: Timestamp,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.record_point_value(attribute, timestamp, value)
    }

    fn record_span(&mut self, attribute: &str, start: Timestamp, end: Timestamp) -> PyResult<()> {
        self.record_span_bounds(attribute, start, end)
    }

    /// Record every event in `batch`, all or nothing.
    fn record_batch(&mut self, py: Python<'_>, batch: PyRef<'_, PyBatch>) -> PyResult<()> {
        let mut core = Batch::new();
        for (attribute, chunk) in &batch.chunks {
            self.push(&mut core, attribute, chunk, py)?;
        }

        let attributes: Vec<&str> = batch
            .chunks
            .iter()
            .map(|(attribute, _)| attribute.as_str())
            .collect();
        self.record(py, core, &attributes)
    }

    fn __eq__(&self, other: PyRef<'_, Self>) -> bool {
        self.inner == other.inner
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let attributes = PyList::new(py, self.attributes())?.repr()?;
        Ok(format!("SequenceSystem({attributes})"))
    }
}

/// Events for one attribute, kept as given until the batch is recorded.
pub enum Chunk {
    Points {
        timestamps: Py<PyAny>,
        values: Py<PyAny>,
    },
    Spans {
        starts: Py<PyAny>,
        ends: Py<PyAny>,
    },
}

/// Events for many sequences of a system, recorded together.
///
/// Converted when recorded, to the value type of the sequence each push is
/// for, so a batch never has to know which system it is for.
#[pyclass(module = "chronoloompy", name = "Batch")]
#[derive(Default)]
pub struct PyBatch {
    chunks: Vec<(String, Chunk)>,
}

#[pymethods]
impl PyBatch {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    fn points<'py>(
        mut slf: PyRefMut<'py, Self>,
        attribute: String,
        timestamps: Py<PyAny>,
        values: Py<PyAny>,
    ) -> PyRefMut<'py, Self> {
        slf.chunks
            .push((attribute, Chunk::Points { timestamps, values }));
        slf
    }

    fn spans<'py>(
        mut slf: PyRefMut<'py, Self>,
        attribute: String,
        starts: Py<PyAny>,
        ends: Py<PyAny>,
    ) -> PyRefMut<'py, Self> {
        slf.chunks.push((attribute, Chunk::Spans { starts, ends }));
        slf
    }

    /// How many events and spans the batch carries, across every attribute.
    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        self.chunks
            .iter()
            .map(|(_, chunk)| match chunk {
                Chunk::Points { timestamps, .. } => timestamps.bind(py).len(),
                Chunk::Spans { starts, .. } => starts.bind(py).len(),
            })
            .sum()
    }
}
