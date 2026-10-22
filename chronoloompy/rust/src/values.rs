//! The value types a point sequence may carry from Python, and getting
//! timestamps and values in from Python objects.
//!
//! The core is generic over a sequence's value type, but a Python class cannot
//! be, so the binding commits to four: `float64`, `int64`, `bool` and `str`.
//! [`PointValues`] owns a sequence of any of them and [`PointRef`] borrows one;
//! [`each_value_type!`] runs one generic body against whichever is held.

use core::mem::size_of;

use pyo3::buffer::{Element, PyBuffer};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyDict;
use pyo3::IntoPyObjectExt;

use chronoloom::{AnyPointSequence, PointValue, Sequence, TimePointEvent, TimePointSequence};
use chronoloom::{TimeIntervalEvent, Timestamp};

use crate::errors::interval_error;
use crate::exports::{numpy, structured_dtype};

/// A value type a point sequence may carry from Python.
pub trait ValueType: PointValue + for<'py> IntoPyObject<'py> {
    /// The name Python spells the type with, as `dtype=` accepts it.
    const NAME: &'static str;
    /// The NumPy format of the value, or `None` when NumPy cannot read the
    /// value in place — a Rust `String` is a pointer, not text.
    const NUMPY_FORMAT: Option<&'static str>;

    /// Pair each timestamp with the value at the same position in `values`,
    /// converted, into the events the core stores.
    fn pair(
        timestamps: &PyBuffer<Timestamp>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<TimePointEvent<Self>>>;

    /// One value, converted.
    fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Self>;

    /// The cache for [`event_dtype`], which must be one per value type.
    fn dtype_cache() -> &'static PyOnceLock<Py<PyAny>>;

    /// Wrap a typed sequence as the binding holds it.
    fn wrap(sequence: TimePointSequence<Self>) -> PointValues;

    /// The values of `events` copied into a new array, for a type NumPy cannot
    /// read in place; `None` for one it can.
    fn copied_values<'py>(
        _py: Python<'py>,
        _events: &[TimePointEvent<Self>],
    ) -> Option<PyResult<Bound<'py, PyAny>>> {
        None
    }
}

/// The structured dtype of one event: its timestamp, and its value when NumPy
/// can read the value in place.
pub fn event_dtype<T: ValueType>(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let dtype = T::dtype_cache().get_or_try_init(py, || {
        let item_size = size_of::<TimePointEvent<T>>();
        let value_offset = size_of::<Timestamp>();
        let dtype = match T::NUMPY_FORMAT {
            Some(format) => structured_dtype(
                py,
                item_size,
                &[("timestamp", "=i8", 0), ("value", format, value_offset)],
            )?,
            None => structured_dtype(py, item_size, &[("timestamp", "=i8", 0)])?,
        };
        Ok::<_, PyErr>(dtype.unbind())
    })?;

    Ok(dtype.bind(py).clone())
}

impl ValueType for f64 {
    const NAME: &'static str = "float64";
    const NUMPY_FORMAT: Option<&'static str> = Some("=f8");

    fn pair(
        timestamps: &PyBuffer<Timestamp>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<TimePointEvent<Self>>> {
        pair_in_place(timestamps, &contiguous(values, "float64")?, |value: f64| {
            value
        })
    }

    fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        value.extract()
    }

    fn dtype_cache() -> &'static PyOnceLock<Py<PyAny>> {
        static CACHE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        &CACHE
    }

    fn wrap(sequence: TimePointSequence<Self>) -> PointValues {
        PointValues::Float(sequence)
    }
}

impl ValueType for i64 {
    const NAME: &'static str = "int64";
    const NUMPY_FORMAT: Option<&'static str> = Some("=i8");

    fn pair(
        timestamps: &PyBuffer<Timestamp>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<TimePointEvent<Self>>> {
        pair_in_place(
            timestamps,
            &integers(values, "int64 values")?,
            |value: i64| value,
        )
    }

    fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        value.extract()
    }

    fn dtype_cache() -> &'static PyOnceLock<Py<PyAny>> {
        static CACHE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        &CACHE
    }

    fn wrap(sequence: TimePointSequence<Self>) -> PointValues {
        PointValues::Integer(sequence)
    }
}

impl ValueType for bool {
    const NAME: &'static str = "bool";
    const NUMPY_FORMAT: Option<&'static str> = Some("?");

    fn pair(
        timestamps: &PyBuffer<Timestamp>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<TimePointEvent<Self>>> {
        // The buffer protocol has no `bool` element, so read the bytes NumPy
        // stores booleans as.
        let bytes = contiguous(values, "bool")?.call_method1("view", ("uint8",))?;
        pair_in_place(timestamps, &bytes, |byte: u8| byte != 0)
    }

    fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        value.extract()
    }

    fn dtype_cache() -> &'static PyOnceLock<Py<PyAny>> {
        static CACHE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        &CACHE
    }

    fn wrap(sequence: TimePointSequence<Self>) -> PointValues {
        PointValues::Boolean(sequence)
    }
}

impl ValueType for String {
    const NAME: &'static str = "str";
    const NUMPY_FORMAT: Option<&'static str> = None;

    fn pair(
        timestamps: &PyBuffer<Timestamp>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<TimePointEvent<Self>>> {
        let texts: Vec<String> = values
            .try_iter()?
            .map(|value| value?.extract())
            .collect::<PyResult<_>>()?;

        // SAFETY: no Python code runs between here and the end of the function.
        let timestamps = unsafe { contiguous_slice(timestamps) };
        same_length(timestamps.len(), texts.len(), "timestamps", "values")?;

        Ok(timestamps
            .iter()
            .zip(texts)
            .map(|(&timestamp, text)| TimePointEvent::new(timestamp, text))
            .collect())
    }

    fn value_from(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        value.extract()
    }

    fn dtype_cache() -> &'static PyOnceLock<Py<PyAny>> {
        static CACHE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        &CACHE
    }

    fn wrap(sequence: TimePointSequence<Self>) -> PointValues {
        PointValues::Text(sequence)
    }

    fn copied_values<'py>(
        py: Python<'py>,
        events: &[TimePointEvent<Self>],
    ) -> Option<PyResult<Bound<'py, PyAny>>> {
        Some(text_array(py, events))
    }
}

/// A point sequence of any supported value type, owned.
#[derive(Debug, Clone, PartialEq)]
pub enum PointValues {
    Float(TimePointSequence<f64>),
    Integer(TimePointSequence<i64>),
    Boolean(TimePointSequence<bool>),
    Text(TimePointSequence<String>),
}

/// A point sequence of any supported value type, borrowed — from a
/// [`PointValues`], or from inside a system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointRef<'a> {
    Float(&'a TimePointSequence<f64>),
    Integer(&'a TimePointSequence<i64>),
    Boolean(&'a TimePointSequence<bool>),
    Text(&'a TimePointSequence<String>),
}

/// Which of the four value types a sequence carries, without borrowing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Float,
    Integer,
    Boolean,
    Text,
}

impl ValueKind {
    /// The value type `dtype=` names.
    pub fn from_name(name: &str) -> PyResult<Self> {
        match name {
            "float64" => Ok(Self::Float),
            "int64" => Ok(Self::Integer),
            "bool" => Ok(Self::Boolean),
            "str" => Ok(Self::Text),
            other => Err(PyValueError::new_err(format!(
                "unsupported dtype `{other}`: expected float64, int64, bool or str"
            ))),
        }
    }
}

/// Run `$body` with the type alias `$type` naming the value type `$kind` is.
macro_rules! with_value_type {
    ($kind:expr, $type:ident => $body:expr) => {
        match $kind {
            $crate::values::ValueKind::Float => {
                type $type = f64;
                $body
            }
            $crate::values::ValueKind::Integer => {
                type $type = i64;
                $body
            }
            $crate::values::ValueKind::Boolean => {
                type $type = bool;
                $body
            }
            $crate::values::ValueKind::Text => {
                type $type = String;
                $body
            }
        }
    };
}
pub(crate) use with_value_type;

/// Run `$body` with `$sequence` bound to whichever typed sequence `$value`
/// holds, where `$value` is a [`PointValues`] or [`PointRef`] (or a reference
/// to one) named by `$kind`.
macro_rules! each_value_type {
    ($kind:ident, $value:expr, $sequence:ident => $body:expr) => {
        match $value {
            $crate::values::$kind::Float($sequence) => $body,
            $crate::values::$kind::Integer($sequence) => $body,
            $crate::values::$kind::Boolean($sequence) => $body,
            $crate::values::$kind::Text($sequence) => $body,
        }
    };
}
pub(crate) use each_value_type;

impl PointValues {
    /// Borrow the sequence.
    pub fn as_ref(&self) -> PointRef<'_> {
        match self {
            Self::Float(sequence) => PointRef::Float(sequence),
            Self::Integer(sequence) => PointRef::Integer(sequence),
            Self::Boolean(sequence) => PointRef::Boolean(sequence),
            Self::Text(sequence) => PointRef::Text(sequence),
        }
    }

    /// Recover the typed sequence from a system member.
    pub fn from_any(sequence: AnyPointSequence) -> Self {
        let sequence = match sequence.downcast::<f64>() {
            Ok(typed) => return Self::Float(typed),
            Err(sequence) => sequence,
        };
        let sequence = match sequence.downcast::<i64>() {
            Ok(typed) => return Self::Integer(typed),
            Err(sequence) => sequence,
        };
        let sequence = match sequence.downcast::<bool>() {
            Ok(typed) => return Self::Boolean(typed),
            Err(sequence) => sequence,
        };
        match sequence.downcast::<String>() {
            Ok(typed) => Self::Text(typed),
            Err(_) => unreachable!("the binding only ever records the four value types"),
        }
    }

    /// Build a sequence of `attribute` from timestamps and values, of the value
    /// type `dtype` names or, without one, the one `values` holds.
    pub fn from_python(
        attribute: &str,
        timestamps: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
        dtype: Option<&str>,
    ) -> PyResult<Self> {
        let kind = match dtype {
            Some(name) => ValueKind::from_name(name)?,
            None => infer_value_type(values)?,
        };

        with_value_type!(kind, T => build::<T>(attribute, timestamps, values))
    }
}

impl From<PointValues> for Sequence {
    fn from(values: PointValues) -> Self {
        each_value_type!(PointValues, values, sequence => Sequence::from(sequence))
    }
}

impl<'a> PointRef<'a> {
    /// Borrow the typed sequence inside a system member.
    pub fn from_any(sequence: &'a AnyPointSequence) -> Self {
        if let Some(typed) = sequence.downcast_ref::<f64>() {
            Self::Float(typed)
        } else if let Some(typed) = sequence.downcast_ref::<i64>() {
            Self::Integer(typed)
        } else if let Some(typed) = sequence.downcast_ref::<bool>() {
            Self::Boolean(typed)
        } else if let Some(typed) = sequence.downcast_ref::<String>() {
            Self::Text(typed)
        } else {
            unreachable!("the binding only ever records the four value types")
        }
    }

    /// Which value type the sequence carries.
    pub fn kind(self) -> ValueKind {
        match self {
            Self::Float(_) => ValueKind::Float,
            Self::Integer(_) => ValueKind::Integer,
            Self::Boolean(_) => ValueKind::Boolean,
            Self::Text(_) => ValueKind::Text,
        }
    }

    /// The name of the value type, as `dtype=` spells it.
    pub fn dtype(self) -> &'static str {
        fn name<T: ValueType>(_: &TimePointSequence<T>) -> &'static str {
            T::NAME
        }
        each_value_type!(PointRef, self, sequence => name(sequence))
    }
}

/// Build a typed sequence, sorting it with the GIL released.
fn build<T: ValueType>(
    attribute: &str,
    timestamps: &Bound<'_, PyAny>,
    values: &Bound<'_, PyAny>,
) -> PyResult<PointValues> {
    let events = events_from::<T>(timestamps, values)?;
    let sequence = timestamps
        .py()
        .detach(|| TimePointSequence::from_events(attribute, events));

    Ok(T::wrap(sequence))
}

/// Pair timestamps with values of type `T`, into the events the core stores.
///
/// Both are read in place from contiguous NumPy arrays — converted first only
/// when they are not already `int64` and `T` — so the events are the one copy
/// made.
pub fn events_from<T: ValueType>(
    timestamps: &Bound<'_, PyAny>,
    values: &Bound<'_, PyAny>,
) -> PyResult<Vec<TimePointEvent<T>>> {
    let timestamps = PyBuffer::<Timestamp>::get(&integers(timestamps, "timestamps")?)?;
    T::pair(&timestamps, values)
}

/// Pair each timestamp with the element at the same position of `values`,
/// converted by `convert`, once `values` is a contiguous array.
fn pair_in_place<E: Element + Copy, T>(
    timestamps: &PyBuffer<Timestamp>,
    values: &Bound<'_, PyAny>,
    convert: impl Fn(E) -> T,
) -> PyResult<Vec<TimePointEvent<T>>> {
    let values = PyBuffer::<E>::get(values)?;

    // SAFETY: no Python code runs between here and the end of the function.
    let (timestamps, values) = unsafe { (contiguous_slice(timestamps), contiguous_slice(&values)) };
    same_length(timestamps.len(), values.len(), "timestamps", "values")?;

    Ok(timestamps
        .iter()
        .zip(values)
        .map(|(&timestamp, &value)| TimePointEvent::new(timestamp, convert(value)))
        .collect())
}

/// Spans from paired starts and ends, each checked to be non-empty.
pub fn spans_from(
    starts: &Bound<'_, PyAny>,
    ends: &Bound<'_, PyAny>,
) -> PyResult<Vec<TimeIntervalEvent<()>>> {
    let starts = PyBuffer::<Timestamp>::get(&integers(starts, "starts")?)?;
    let ends = PyBuffer::<Timestamp>::get(&integers(ends, "ends")?)?;

    // SAFETY: no Python code runs between here and the end of the function.
    let (starts, ends) = unsafe { (contiguous_slice(&starts), contiguous_slice(&ends)) };
    same_length(starts.len(), ends.len(), "starts", "ends")?;

    starts
        .iter()
        .zip(ends)
        .map(|(&start, &end)| TimeIntervalEvent::span(start, end).map_err(interval_error))
        .collect()
}

/// Spans from an `(n, 2)` array of `[start, end]` rows.
pub fn spans_from_rows(rows: &Bound<'_, PyAny>) -> PyResult<Vec<TimeIntervalEvent<()>>> {
    let array = integers(rows, "spans")?;
    let size: usize = array.getattr("size")?.extract()?;
    let shape: Vec<usize> = array.getattr("shape")?.extract()?;
    if size > 0 && (shape.len() != 2 || shape[1] != 2) {
        return Err(PyValueError::new_err(format!(
            "spans must have shape (n, 2), not {shape:?}"
        )));
    }
    let bounds = PyBuffer::<Timestamp>::get(&array)?;

    // SAFETY: no Python code runs between here and the end of the function.
    unsafe { contiguous_slice(&bounds) }
        .chunks_exact(2)
        .map(|row| TimeIntervalEvent::span(row[0], row[1]).map_err(interval_error))
        .collect()
}

/// Refuse paired inputs of different lengths.
fn same_length(left: usize, right: usize, left_name: &str, right_name: &str) -> PyResult<()> {
    if left == right {
        Ok(())
    } else {
        Err(PyValueError::new_err(format!(
            "{left} {left_name} but {right} {right_name}"
        )))
    }
}

/// The elements of a buffer obtained from `numpy.ascontiguousarray`.
///
/// # Safety
///
/// No Python code may run while the slice is alive. Holding the GIL then means
/// nothing else can change the memory under it — the promise pyo3 leaves to
/// the caller by offering only a raw pointer.
unsafe fn contiguous_slice<E: Element>(buffer: &PyBuffer<E>) -> &[E] {
    let slice = buffer
        .as_slice_ptr()
        .expect("numpy.ascontiguousarray returns a C-contiguous array");

    // SAFETY: valid for as long as `buffer` holds the export, and the caller
    // promises nothing changes the memory meanwhile.
    unsafe { slice.as_ref() }
}

/// `values` as a contiguous `int64` array, refusing anything but integers.
fn integers<'py>(values: &Bound<'py, PyAny>, what: &str) -> PyResult<Bound<'py, PyAny>> {
    let array = numpy(values.py())?.getattr("asarray")?.call1((values,))?;
    let kind: String = array.getattr("dtype")?.getattr("kind")?.extract()?;
    let size: usize = array.getattr("size")?.extract()?;
    if size > 0 && kind != "i" && kind != "u" {
        return Err(PyTypeError::new_err(format!(
            "{what} must be integers, not {}",
            array.getattr("dtype")?
        )));
    }

    contiguous(&array, "int64")
}

/// `values` as a contiguous NumPy array of `dtype`, converting only if needed.
fn contiguous<'py>(values: &Bound<'py, PyAny>, dtype: &str) -> PyResult<Bound<'py, PyAny>> {
    let keywords = PyDict::new(values.py());
    keywords.set_item("dtype", dtype)?;

    numpy(values.py())?
        .getattr("ascontiguousarray")?
        .call((values,), Some(&keywords))
}

/// The value type `values` holds, by the dtype NumPy gives it.
fn infer_value_type(values: &Bound<'_, PyAny>) -> PyResult<ValueKind> {
    let array = numpy(values.py())?.getattr("asarray")?.call1((values,))?;
    let dtype = array.getattr("dtype")?;
    let kind: String = dtype.getattr("kind")?.extract()?;

    match kind.as_str() {
        "f" => Ok(ValueKind::Float),
        "i" | "u" => Ok(ValueKind::Integer),
        "b" => Ok(ValueKind::Boolean),
        "U" | "T" => Ok(ValueKind::Text),
        _ => Err(PyTypeError::new_err(format!(
            "values of dtype {dtype} are not supported: expected floats, integers, booleans or \
             strings, or pass dtype="
        ))),
    }
}

/// A copy of string values, as a NumPy array of `StringDType`.
fn text_array<'py>(
    py: Python<'py>,
    values: &[TimePointEvent<String>],
) -> PyResult<Bound<'py, PyAny>> {
    let numpy = numpy(py)?;
    let strings: Vec<&str> = values.iter().map(|event| event.value().as_str()).collect();
    let keywords = PyDict::new(py);
    keywords.set_item(
        "dtype",
        numpy.getattr("dtypes")?.getattr("StringDType")?.call0()?,
    )?;

    numpy.getattr("array")?.call((strings,), Some(&keywords))
}

/// One event's value as a Python object.
pub fn value_object<'py, T: ValueType>(
    py: Python<'py>,
    event: &TimePointEvent<T>,
) -> PyResult<Bound<'py, PyAny>> {
    event.value().clone().into_bound_py_any(py)
}
