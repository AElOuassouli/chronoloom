//! Read-only NumPy views over memory the core owns.
//!
//! A view is an [`Export`]: a small object exposing a slice of a sequence's
//! events through the buffer protocol, which `numpy.frombuffer` wraps without
//! copying. The export holds a strong reference to the Python object owning
//! the memory, so the memory outlives every view, and a [`Lease`] on that
//! memory's [`ExportCounter`]. Whatever could move or free the memory checks
//! the counter first and refuses with `BufferError` while any lease is alive,
//! the way `bytearray` refuses to resize while exported. NumPy keeps the export
//! alive for exactly as long as some array uses it, so the last view to be
//! collected is what frees the memory for change again.

use core::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pyo3::exceptions::PyBufferError;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyDict;

use crate::errors::exported;

/// How many leases are alive on one sequence's memory.
#[derive(Debug, Default, Clone)]
pub struct ExportCounter(Arc<AtomicUsize>);

impl ExportCounter {
    /// Refuse, naming `label`, if any view of the memory is alive.
    pub fn ensure_unexported(&self, label: &str) -> PyResult<()> {
        if self.0.load(Ordering::Acquire) == 0 {
            Ok(())
        } else {
            Err(exported(label))
        }
    }

    fn lease(&self) -> Lease {
        self.0.fetch_add(1, Ordering::AcqRel);
        Lease(Arc::clone(&self.0))
    }
}

/// One view's claim on the memory, released when the view is dropped.
struct Lease(Arc<AtomicUsize>);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A slice of a sequence's memory, exposed read-only through the buffer
/// protocol. Never seen from Python except as an array's `base`.
#[pyclass(frozen, module = "chronoloompy._core")]
pub struct Export {
    /// The slice's address, as an integer so the export is `Send` and `Sync`.
    /// Stays valid for the export's lifetime: `owner` keeps the memory alive,
    /// and `_lease` keeps it from being moved or freed.
    address: usize,
    length: usize,
    _owner: Py<PyAny>,
    _lease: Lease,
}

#[pymethods]
impl Export {
    unsafe fn __getbuffer__(
        slf: Bound<'_, Self>,
        view: *mut ffi::Py_buffer,
        flags: c_int,
    ) -> PyResult<()> {
        if view.is_null() {
            return Err(PyBufferError::new_err("the buffer view is null"));
        }
        if (flags & ffi::PyBUF_WRITABLE) == ffi::PyBUF_WRITABLE {
            return Err(PyBufferError::new_err("chronoloompy views are read-only"));
        }

        let export = slf.get();
        // SAFETY: `view` is non-null and CPython hands us a buffer to fill. The
        // memory it describes stays valid and unchanged for the export's
        // lifetime, and `obj` keeps the export alive for the buffer's lifetime.
        unsafe {
            (*view).buf = export.address as *mut c_void;
            (*view).len = isize::try_from(export.length).expect("a Vec never exceeds isize::MAX");
            (*view).readonly = 1;
            (*view).itemsize = 1;
            (*view).format = if (flags & ffi::PyBUF_FORMAT) == ffi::PyBUF_FORMAT {
                c"B".as_ptr().cast_mut()
            } else {
                core::ptr::null_mut()
            };
            (*view).ndim = 1;
            (*view).shape = if (flags & ffi::PyBUF_ND) == ffi::PyBUF_ND {
                &raw mut (*view).len
            } else {
                core::ptr::null_mut()
            };
            (*view).strides = if (flags & ffi::PyBUF_STRIDES) == ffi::PyBUF_STRIDES {
                &raw mut (*view).itemsize
            } else {
                core::ptr::null_mut()
            };
            (*view).suboffsets = core::ptr::null_mut();
            (*view).internal = core::ptr::null_mut();
            (*view).obj = slf.into_any().into_ptr();
        }

        Ok(())
    }

    unsafe fn __releasebuffer__(&self, _view: *mut ffi::Py_buffer) {
        // The format string is static, so there is nothing to free.
    }
}

/// A read-only NumPy array of `dtype` over `slice`, which `owner` owns and
/// `counter` guards.
pub fn view<'py, T>(
    owner: &Bound<'py, PyAny>,
    counter: &ExportCounter,
    slice: &[T],
    dtype: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = owner.py();
    let export = Export {
        address: slice.as_ptr() as usize,
        length: core::mem::size_of_val(slice),
        _owner: owner.clone().unbind(),
        _lease: counter.lease(),
    };

    let keywords = PyDict::new(py);
    keywords.set_item("dtype", dtype)?;
    numpy(py)?
        .getattr("frombuffer")?
        .call((Bound::new(py, export)?,), Some(&keywords))
}

/// A structured NumPy dtype describing events of `item_size` bytes, with the
/// given `(name, format, offset)` fields.
pub fn structured_dtype<'py>(
    py: Python<'py>,
    item_size: usize,
    fields: &[(&str, &str, usize)],
) -> PyResult<Bound<'py, PyAny>> {
    let description = PyDict::new(py);
    description.set_item(
        "names",
        fields.iter().map(|field| field.0).collect::<Vec<_>>(),
    )?;
    description.set_item(
        "formats",
        fields.iter().map(|field| field.1).collect::<Vec<_>>(),
    )?;
    description.set_item(
        "offsets",
        fields.iter().map(|field| field.2).collect::<Vec<_>>(),
    )?;
    description.set_item("itemsize", item_size)?;

    numpy(py)?.getattr("dtype")?.call1((description,))
}

/// The `numpy` module, imported once.
pub fn numpy(py: Python<'_>) -> PyResult<&Bound<'_, PyModule>> {
    static NUMPY: PyOnceLock<Py<PyModule>> = PyOnceLock::new();
    NUMPY
        .get_or_try_init(py, || py.import("numpy").map(Bound::unbind))
        .map(|module| module.bind(py))
}
