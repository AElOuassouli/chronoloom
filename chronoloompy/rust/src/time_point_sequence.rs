//! A sequence of point events, of its own or inside a system.

use std::ops::Bound as RangeBound;

use pyo3::exceptions::{PyIndexError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use chronoloom::{Sequence, TimePointEvent, TimePointSequence, Timestamp};

use crate::errors::{belongs_to_system, missing_member};
use crate::exports::{view, ExportCounter};
use crate::primitives::PyTimePointEvent;
use crate::sequence_system::{Chunk, PySequenceSystem};
use crate::storage::Storage;
use crate::values::{each_value_type, event_dtype, events_from, value_object};
use crate::values::{PointRef, PointValues, ValueType};

/// A time-ordered sequence of point events, held in Rust.
#[pyclass(module = "chronoloompy", name = "TimePointSequence")]
pub struct PyTimePointSequence {
    storage: Storage<PointValues>,
}

impl PyTimePointSequence {
    pub fn owned(values: PointValues) -> Self {
        Self {
            storage: Storage::owned(values),
        }
    }

    pub fn member(system: Py<PySequenceSystem>, attribute: String) -> Self {
        Self {
            storage: Storage::Member { system, attribute },
        }
    }

    /// Run `read` against the events, wherever they live.
    fn read<R>(
        &self,
        py: Python<'_>,
        read: impl FnOnce(PointRef<'_>) -> PyResult<R>,
    ) -> PyResult<R> {
        match &self.storage {
            Storage::Owned { sequence, .. } => read(sequence.as_ref()),
            Storage::Member { system, attribute } => match system.borrow(py).inner.get(attribute) {
                Some(Sequence::TimePoint(points)) => read(PointRef::from_any(points)),
                _ => Err(missing_member(attribute)),
            },
        }
    }

    /// Run `read` against the events, with the object owning their memory and
    /// the counter guarding it, for handing out views.
    fn exporting<'py, R>(
        slf: &Bound<'py, Self>,
        read: impl FnOnce(&Bound<'py, PyAny>, &ExportCounter, PointRef<'_>) -> PyResult<R>,
    ) -> PyResult<R> {
        let this = slf.borrow();
        match &this.storage {
            Storage::Owned { sequence, exports } => read(slf.as_any(), exports, sequence.as_ref()),
            Storage::Member { system, attribute } => {
                let system = system.bind(slf.py());
                let guard = system.borrow();
                match guard.inner.get(attribute) {
                    Some(Sequence::TimePoint(points)) => read(
                        system.as_any(),
                        &guard.counter(attribute),
                        PointRef::from_any(points),
                    ),
                    _ => Err(missing_member(attribute)),
                }
            }
        }
    }

    /// The events of a sequence of its own, for changing — refused while any
    /// view of them is alive.
    fn owned_mut(&mut self) -> PyResult<&mut PointValues> {
        match &mut self.storage {
            Storage::Owned { sequence, exports } => {
                exports.ensure_unexported(
                    &each_value_type!(PointValues, &*sequence, typed => typed.label()),
                )?;
                Ok(sequence)
            }
            Storage::Member { attribute, .. } => Err(belongs_to_system(attribute)),
        }
    }

    /// The attribute of a sequence of its own, refusing one already in a
    /// system.
    pub fn owned_label(&self) -> PyResult<String> {
        match &self.storage {
            Storage::Owned { sequence, .. } => {
                Ok(each_value_type!(PointValues, sequence, typed => typed.label()))
            }
            Storage::Member { attribute, .. } => Err(PyTypeError::new_err(format!(
                "`{attribute}` already belongs to a SequenceSystem"
            ))),
        }
    }

    /// Hand the events to `system`, becoming a handle onto them there.
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
}

/// A read-only view of `events`: the structured array, with a `value` field
/// only when NumPy can read the values in place.
fn events_view<'py, T: ValueType>(
    owner: &Bound<'py, PyAny>,
    counter: &ExportCounter,
    events: &[TimePointEvent<T>],
) -> PyResult<Bound<'py, PyAny>> {
    view(owner, counter, events, &event_dtype::<T>(owner.py())?)
}

/// The values of `events`: a view when NumPy can read them in place, a copy
/// when it cannot.
fn values_of<'py, T: ValueType>(
    owner: &Bound<'py, PyAny>,
    counter: &ExportCounter,
    events: &[TimePointEvent<T>],
) -> PyResult<Bound<'py, PyAny>> {
    match T::copied_values(owner.py(), events) {
        Some(copy) => copy,
        None => events_view(owner, counter, events)?.get_item("value"),
    }
}

fn event_object<T: ValueType>(
    py: Python<'_>,
    event: &TimePointEvent<T>,
) -> PyResult<PyTimePointEvent> {
    Ok(PyTimePointEvent {
        inner: TimePointEvent::new(event.timestamp(), value_object(py, event)?.unbind()),
    })
}

fn optional_event<T: ValueType>(
    py: Python<'_>,
    event: Option<&TimePointEvent<T>>,
) -> PyResult<Option<PyTimePointEvent>> {
    event.map(|event| event_object(py, event)).transpose()
}

fn event_list<'py, T: ValueType>(
    py: Python<'py>,
    events: &[TimePointEvent<T>],
) -> PyResult<Bound<'py, PyList>> {
    let objects = events
        .iter()
        .map(|event| event_object(py, event))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, objects)
}

fn insert_value<T: ValueType>(
    sequence: &mut TimePointSequence<T>,
    timestamp: Timestamp,
    value: &Bound<'_, PyAny>,
) -> PyResult<()> {
    sequence.insert(TimePointEvent::new(timestamp, T::value_from(value)?));
    Ok(())
}

fn extend_values<T: ValueType>(
    sequence: &mut TimePointSequence<T>,
    timestamps: &Bound<'_, PyAny>,
    values: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let events = events_from::<T>(timestamps, values)?;
    timestamps.py().detach(|| sequence.extend(events));
    Ok(())
}

#[pymethods]
impl PyTimePointSequence {
    #[new]
    #[pyo3(signature = (attribute, timestamps = None, values = None, *, dtype = None))]
    fn new(
        py: Python<'_>,
        attribute: &str,
        timestamps: Option<&Bound<'_, PyAny>>,
        values: Option<&Bound<'_, PyAny>>,
        dtype: Option<&str>,
    ) -> PyResult<Self> {
        let empty = PyTuple::empty(py).into_any();
        let values = PointValues::from_python(
            attribute,
            timestamps.unwrap_or(&empty),
            values.unwrap_or(&empty),
            dtype,
        )?;

        Ok(Self::owned(values))
    }

    /// What the sequence is a record of.
    #[getter]
    fn attribute(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |points| {
            Ok(each_value_type!(PointRef, points, typed => typed.label()))
        })
    }

    /// The attribute written out — the text a system files the sequence under.
    #[getter]
    fn label(&self, py: Python<'_>) -> PyResult<String> {
        self.attribute(py)
    }

    /// The value type the events carry: `float64`, `int64`, `bool` or `str`.
    #[getter]
    fn dtype(&self, py: Python<'_>) -> PyResult<&'static str> {
        self.read(py, |points| Ok(points.dtype()))
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        self.read(py, |points| {
            Ok(each_value_type!(PointRef, points, typed => typed.len()))
        })
    }

    fn instant_count(&self, py: Python<'_>) -> PyResult<usize> {
        self.read(py, |points| {
            Ok(each_value_type!(PointRef, points, typed => typed.instant_count()))
        })
    }

    /// Every timestamp, as a read-only view of the sequence's memory.
    #[getter]
    fn timestamps<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        Self::exporting(slf, |owner, counter, points| {
            each_value_type!(PointRef, points, typed => {
                events_view(owner, counter, typed.as_slice())?.get_item("timestamp")
            })
        })
    }

    /// Every value: a read-only view for numbers and booleans, a copy for text.
    #[getter]
    fn values<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        Self::exporting(slf, |owner, counter, points| {
            each_value_type!(PointRef, points, typed => {
                values_of(owner, counter, typed.as_slice())
            })
        })
    }

    /// The events as a read-only structured array of `timestamp` and `value`.
    fn to_numpy<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        Self::exporting(slf, |owner, counter, points| {
            if let PointRef::Text(_) = points {
                return Err(PyTypeError::new_err(
                    "text values cannot be viewed in place; read `timestamps` and `values` instead",
                ));
            }
            each_value_type!(PointRef, points, typed => {
                events_view(owner, counter, typed.as_slice())
            })
        })
    }

    /// The timestamps and values in `[start, end)`; either bound may be
    /// omitted.
    #[pyo3(signature = (start = None, end = None))]
    fn range<'py>(
        slf: &Bound<'py, Self>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    ) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)> {
        let bounds = (
            start.map_or(RangeBound::Unbounded, RangeBound::Included),
            end.map_or(RangeBound::Unbounded, RangeBound::Excluded),
        );
        let empty = matches!((start, end), (Some(start), Some(end)) if start > end);

        Self::exporting(slf, |owner, counter, points| {
            each_value_type!(PointRef, points, typed => {
                let window = if empty { &typed.as_slice()[..0] } else { typed.range(bounds) };
                let timestamps = events_view(owner, counter, window)?.get_item("timestamp")?;
                Ok((timestamps, values_of(owner, counter, window)?))
            })
        })
    }

    /// Every event at exactly `timestamp`, in the order they were added.
    fn get<'py>(&self, py: Python<'py>, timestamp: Timestamp) -> PyResult<Bound<'py, PyList>> {
        self.read(py, |points| {
            each_value_type!(PointRef, points, typed => event_list(py, typed.get(timestamp)))
        })
    }

    fn contains(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<bool> {
        self.read(py, |points| {
            Ok(each_value_type!(PointRef, points, typed => typed.contains(timestamp)))
        })
    }

    fn first(&self, py: Python<'_>) -> PyResult<Option<PyTimePointEvent>> {
        self.read(
            py,
            |points| each_value_type!(PointRef, points, typed => optional_event(py, typed.first())),
        )
    }

    fn last(&self, py: Python<'_>) -> PyResult<Option<PyTimePointEvent>> {
        self.read(
            py,
            |points| each_value_type!(PointRef, points, typed => optional_event(py, typed.last())),
        )
    }

    /// The last event at or before `timestamp`.
    fn before(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<Option<PyTimePointEvent>> {
        self.read(py, |points| {
            each_value_type!(PointRef, points, typed => {
                optional_event(py, typed.before(timestamp))
            })
        })
    }

    /// The first event at or after `timestamp`.
    fn after(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<Option<PyTimePointEvent>> {
        self.read(py, |points| {
            each_value_type!(PointRef, points, typed => {
                optional_event(py, typed.after(timestamp))
            })
        })
    }

    /// The closest event in either direction, the earlier one on a tie.
    fn nearest(&self, py: Python<'_>, timestamp: Timestamp) -> PyResult<Option<PyTimePointEvent>> {
        self.read(py, |points| {
            each_value_type!(PointRef, points, typed => {
                optional_event(py, typed.nearest(timestamp))
            })
        })
    }

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<PyTimePointEvent> {
        self.read(py, |points| {
            each_value_type!(PointRef, points, typed => {
                let position = if index < 0 {
                    index.checked_add_unsigned(typed.len())
                } else {
                    Some(index)
                };
                position
                    .and_then(|position| usize::try_from(position).ok())
                    .and_then(|position| typed.nth(position))
                    .ok_or_else(|| PyIndexError::new_err("event index out of range"))
                    .and_then(|event| event_object(py, event))
            })
        })
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let events = self.read(
            py,
            |points| each_value_type!(PointRef, points, typed => event_list(py, typed.as_slice())),
        )?;
        events.as_any().try_iter().map(Bound::into_any)
    }

    fn insert(
        &mut self,
        py: Python<'_>,
        timestamp: Timestamp,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if let Storage::Member { system, attribute } = &self.storage {
            return system
                .borrow_mut(py)
                .record_point_value(attribute, timestamp, value);
        }

        each_value_type!(PointValues, self.owned_mut()?, typed => {
            insert_value(typed, timestamp, value)
        })
    }

    fn extend(
        &mut self,
        py: Python<'_>,
        timestamps: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if let Storage::Member { system, attribute } = &self.storage {
            let chunk = Chunk::Points {
                timestamps: timestamps.clone().unbind(),
                values: values.clone().unbind(),
            };
            return system.borrow_mut(py).record_chunk(py, attribute, chunk);
        }

        each_value_type!(PointValues, self.owned_mut()?, typed => {
            extend_values(typed, timestamps, values)
        })
    }

    /// Take every event at `timestamp` out, returning them.
    fn remove<'py>(
        &mut self,
        py: Python<'py>,
        timestamp: Timestamp,
    ) -> PyResult<Bound<'py, PyList>> {
        each_value_type!(PointValues, self.owned_mut()?, typed => {
            event_list(py, &typed.remove(timestamp))
        })
    }

    fn clear(&mut self) -> PyResult<()> {
        each_value_type!(PointValues, self.owned_mut()?, typed => typed.clear());
        Ok(())
    }

    fn set_attribute(&mut self, attribute: &str) -> PyResult<()> {
        match &mut self.storage {
            Storage::Owned { sequence, .. } => {
                each_value_type!(PointValues, sequence, typed => typed.set_attribute(attribute));
                Ok(())
            }
            Storage::Member { attribute, .. } => Err(belongs_to_system(attribute)),
        }
    }

    fn __eq__(&self, py: Python<'_>, other: PyRef<'_, Self>) -> PyResult<bool> {
        self.read(py, |left| other.read(py, |right| Ok(left == right)))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |points| {
            let (label, length) =
                each_value_type!(PointRef, points, typed => (typed.label(), typed.len()));
            Ok(format!(
                "<TimePointSequence '{label}': {length} {} events>",
                points.dtype()
            ))
        })
    }
}
