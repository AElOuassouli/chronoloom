//! Where a sequence object's events live: in the object itself, or inside a
//! system the object is a handle into.

use pyo3::prelude::*;

use crate::exports::ExportCounter;
use crate::sequence_system::PySequenceSystem;

/// Where one sequence object's events live.
///
/// A handle never copies the member it stands for: every read borrows the
/// system and looks the member up by attribute, so a handle always reads what
/// the system currently files under that attribute.
pub enum Storage<O> {
    /// A sequence of its own, with the counter guarding its memory.
    Owned { sequence: O, exports: ExportCounter },
    /// The member filed under `attribute` in `system`.
    Member {
        system: Py<PySequenceSystem>,
        attribute: String,
    },
}

impl<O> Storage<O> {
    /// A sequence of its own, with no views of it yet.
    pub fn owned(sequence: O) -> Self {
        Self::Owned {
            sequence,
            exports: ExportCounter::default(),
        }
    }
}
