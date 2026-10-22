//! A timeline of one state, of its own or inside a system.

use pyo3::exceptions::{PyIndexError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use chronoloom::{Sequence, TimeIntervalSequence, Timestamp};

use crate::errors::{belongs_to_system, interval_error, missing_member};
use crate::exports::{numpy, view, ExportCounter};
use crate::primitives::PyTimeIntervalEvent;
use crate::sequence_system::{Chunk, PySequenceSystem};
use crate::storage::Storage;
use crate::values::{spans_from, spans_from_rows};

/// The spans during which one state was active, held in Rust and kept
/// normalized.
#[pyclass(module = "chronoloompy", name = "TimeIntervalSequence")]
pub struct PyTimeIntervalSequence {
    storage: Storage<TimeIntervalSequence>,
}

impl PyTimeIntervalSequence {
    pub fn owned(timeline: TimeIntervalSequence) -> Self {
        Self {
            storage: Storage::owned(timeline),
        }
    }

    pub fn member(system: Py<PySequenceSystem>, attribute: String) -> Self {
        Self {
            storage: Storage::Member { system, attribute },
        }
    }

    /// Run `read` against the timeline, wherever it lives.
    fn read<R>(
        &self,
        py: Python<'_>,
        read: impl FnOnce(&TimeIntervalSequence) -> PyResult<R>,
    ) -> PyResult<R> {
        match &self.storage {
            Storage::Owned { sequence, .. } => read(sequence),
            Storage::Member { system, attribute } => match system.borrow(py).inner.get(attribute) {
                Some(Sequence::TimeInterval(timeline)) => read(timeline),
                _ => Err(missing_member(attribute)),
            },
        }
    }

    /// Run `read` against the timeline, with the object owning its memory and
    /// the counter guarding it, for handing out views.
    fn exporting<'py, R>(
        slf: &Bound<'py, Self>,
        read: impl FnOnce(&Bound<'py, PyAny>, &ExportCounter, &TimeIntervalSequence) -> PyResult<R>,
    ) -> PyResult<R> {
        let this = slf.borrow();
        match &this.storage {
            Storage::Owned { sequence, exports } => read(slf.as_any(), exports, sequence),
            Storage::Member { system, attribute } => {
                let system = system.bind(slf.py());
                let guard = system.borrow();
                match guard.inner.get(attribute) {
                    Some(Sequence::TimeInterval(timeline)) => {
                        read(system.as_any(), &guard.counter(attribute), timeline)
                    }
                    _ => Err(missing_member(attribute)),
                }
            }
        }
    }

    /// The timeline of its own, for changing — refused while any view of it is
    /// alive.
    fn owned_mut(&mut self) -> PyResult<&mut TimeIntervalSequence> {
        match &mut self.storage {
            Storage::Owned { sequence, exports } => {
                exports.ensure_unexported(&sequence.label())?;
                Ok(sequence)
            }
            Storage::Member { attribute, .. } => Err(belongs_to_system(attribute)),
        }
    }

    /// The label of a timeline of its own, refusing one already in a system.
    pub fn owned_label(&self) -> PyResult<String> {
        match &self.storage {
            Storage::Owned { sequence, .. } => Ok(sequence.label()),
            Storage::Member { attribute, .. } => Err(PyTypeError::new_err(format!(
                "`{attribute}` already belongs to a SequenceSystem"
            ))),
        }
    }

    /// Hand the timeline to `system`, becoming a handle onto it there.
    pub fn move_into(&mut self, system: Py<PySequenceSystem>) -> PyResult<Sequence> {
        let attribute = self.owned_label()?;
        let Storage::Owned { exports, .. } = &self.storage else {
            unreachable!("owned_label refuses a member");
        };
        exports.ensure_unexported(&attribute)?;

        let previous = std::mem::replace(&mut self.storage, Storage::Member { system, attribute });
        let Storage::Owned { sequence, .. } = previous else {
            unreachable!("checked to be owned above");
        };

        Ok(sequence.into())
    }

    /// Combine two timelines, wherever each lives, with the GIL released.
    fn combine(
        &self,
        py: Python<'_>,
        other: &Self,
        operation: fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence,
    ) -> PyResult<Self> {
        let combined = self.read(py, |left| {
            other.read(py, |right| Ok(py.detach(|| operation(left, right))))
        })?;

        Ok(Self::owned(combined))
    }
}

fn span_list<'py>(
    py: Python<'py>,
    timeline: &TimeIntervalSequence,
) -> PyResult<Bound<'py, PyList>> {
    PyList::new(
        py,
        timeline
            .iter()
            .map(|span| PyTimeIntervalEvent::from_span(py, span)),
    )
}

#[pymethods]
impl PyTimeIntervalSequence {
    #[new]
    #[pyo3(signature = (attribute, starts = None, ends = None))]
    fn new(
        py: Python<'_>,
        attribute: &str,
        starts: Option<&Bound<'_, PyAny>>,
        ends: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let empty = PyTuple::empty(py).into_any();
        let spans = spans_from(starts.unwrap_or(&empty), ends.unwrap_or(&empty))?;
        let timeline = py.detach(|| TimeIntervalSequence::from_spans(attribute, spans));

        Ok(Self::owned(timeline))
    }

    /// Build a timeline from an `(n, 2)` array of `[start, end]` rows, such as
    /// another timeline's `spans`.
    #[staticmethod]
    fn from_spans(py: Python<'_>, attribute: &str, spans: &Bound<'_, PyAny>) -> PyResult<Self> {
        let spans = spans_from_rows(spans)?;
        let timeline = py.detach(|| TimeIntervalSequence::from_spans(attribute, spans));

        Ok(Self::owned(timeline))
    }

    /// The state this is the timeline of, as the expression that produced it.
    #[getter]
    fn attribute(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |timeline| Ok(timeline.attribute().to_string()))
    }

    /// The attribute with its transformation — the text a system files the
    /// timeline under.
    #[getter]
    fn label(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |timeline| Ok(timeline.label()))
    }

    /// The `(alpha, beta)` shift `transform` has applied so far.
    #[getter]
    fn transformation(&self, py: Python<'_>) -> PyResult<(Timestamp, Timestamp)> {
        self.read(py, |timeline| Ok(timeline.transformation()))
    }

    /// Every span as an `(n, 2)` read-only view of `[start, end)` rows.
    #[getter]
    fn spans<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        Self::exporting(slf, |owner, counter, timeline| {
            let int64 = numpy(owner.py())?.getattr("int64")?;
            view(owner, counter, timeline.as_slice(), &int64)?.call_method1("reshape", (-1, 2))
        })
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        self.read(py, |timeline| Ok(timeline.len()))
    }

    /// How much time the timeline covers in total.
    fn active_duration(&self, py: Python<'_>) -> PyResult<u64> {
        self.read(py, |timeline| Ok(timeline.active_duration()))
    }

    /// The span covering `timestamp`, if any.
    fn at(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<Option<PyTimeIntervalEvent>> {
        self.read(py, |timeline| {
            Ok(timeline
                .at(timestamp)
                .map(|span| PyTimeIntervalEvent::from_span(py, span)))
        })
    }

    fn contains(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<bool> {
        self.read(py, |timeline| Ok(timeline.contains(timestamp)))
    }

    /// Whether both timelines cover the same instants, whatever they describe.
    fn covers_same(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<bool> {
        self.read(py, |left| {
            other.read(py, |right| Ok(left.covers_same(right)))
        })
    }

    /// The timeline with every `[s, e)` moved to `[s + alpha, e + beta)`.
    fn transform(&self, py: Python<'_>, alpha: Timestamp, beta: Timestamp) -> PyResult<Self> {
        let transformed = self.read(py, |timeline| {
            py.detach(|| timeline.transform(alpha, beta))
                .map_err(interval_error)
        })?;

        Ok(Self::owned(transformed))
    }

    fn union(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.combine(py, &other, TimeIntervalSequence::union)
    }

    fn intersection(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.combine(py, &other, TimeIntervalSequence::intersection)
    }

    fn difference(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.combine(py, &other, TimeIntervalSequence::difference)
    }

    fn symmetric_difference(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.combine(py, &other, TimeIntervalSequence::symmetric_difference)
    }

    fn __or__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.union(py, other)
    }

    fn __and__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.intersection(py, other)
    }

    fn __sub__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.difference(py, other)
    }

    fn __xor__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<Self> {
        self.symmetric_difference(py, other)
    }

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<PyTimeIntervalEvent> {
        self.read(py, |timeline| {
            let position = if index < 0 {
                index.checked_add_unsigned(timeline.len())
            } else {
                Some(index)
            };
            position
                .and_then(|position| usize::try_from(position).ok())
                .and_then(|position| timeline.nth(position))
                .map(|span| PyTimeIntervalEvent::from_span(py, span))
                .ok_or_else(|| PyIndexError::new_err("span index out of range"))
        })
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let spans = self.read(py, |timeline| span_list(py, timeline))?;
        spans.as_any().try_iter().map(Bound::into_any)
    }

    /// Add the span `[start, end)`, merging it into what is already covered.
    fn insert(&mut self, py: Python<'_>, start: Timestamp, end: Timestamp) -> PyResult<()> {
        if let Storage::Member { system, attribute } = &self.storage {
            return system
                .borrow_mut(py)
                .record_span_bounds(attribute, start, end);
        }

        let span = chronoloom::TimeIntervalEvent::span(start, end).map_err(interval_error)?;
        self.owned_mut()?.insert(span);
        Ok(())
    }

    fn extend(
        &mut self,
        py: Python<'_>,
        starts: &Bound<'_, PyAny>,
        ends: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if let Storage::Member { system, attribute } = &self.storage {
            let chunk = Chunk::Spans {
                starts: starts.clone().unbind(),
                ends: ends.clone().unbind(),
            };
            return system.borrow_mut(py).record_chunk(py, attribute, chunk);
        }

        let spans = spans_from(starts, ends)?;
        let timeline = self.owned_mut()?;
        py.detach(|| timeline.extend(spans));
        Ok(())
    }

    /// Take the span at `index` out, returning it.
    fn remove(&mut self, py: Python<'_>, index: usize) -> PyResult<PyTimeIntervalEvent> {
        let timeline = self.owned_mut()?;
        if index >= timeline.len() {
            return Err(PyIndexError::new_err("span index out of range"));
        }

        Ok(PyTimeIntervalEvent::from_span(py, &timeline.remove(index)))
    }

    fn clear(&mut self) -> PyResult<()> {
        self.owned_mut()?.clear();
        Ok(())
    }

    fn set_attribute(&mut self, attribute: &str) -> PyResult<()> {
        match &mut self.storage {
            Storage::Owned { sequence, .. } => {
                sequence.set_attribute(attribute);
                Ok(())
            }
            Storage::Member { attribute, .. } => Err(belongs_to_system(attribute)),
        }
    }

    fn __eq__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<bool> {
        self.read(py, |left| other.read(py, |right| Ok(left == right)))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |timeline| {
            Ok(format!(
                "<TimeIntervalSequence '{}': {} spans>",
                timeline.label(),
                timeline.len()
            ))
        })
    }
}
