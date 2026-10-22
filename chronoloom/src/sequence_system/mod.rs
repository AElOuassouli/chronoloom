//! Systems of sequences: everything recorded about one subject, side by side.
//!
//! A [`SequenceSystem`] is to sequences what a sensor system is to sensors. A
//! machine has a thermometer reporting `f64` readings, a logger reporting
//! `String` lines, and an uptime state that is either on or off — three
//! sequences of two shapes and three value types, describing one subject. The
//! system records one sequence per attribute, and records events into each as
//! the events arrive.
//!
//! ```
//! use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
//! use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};
//! use chronoloom::sequence_system::{Batch, SequenceSystem};
//!
//! // Declare each sensor once, by the attribute it reports.
//! let mut machine = SequenceSystem::new();
//! machine.insert(TimePointSequence::<f64>::new("temperature"));
//! machine.insert(TimePointSequence::<String>::new("log"));
//! machine.insert(TimeIntervalSequence::new("up"));
//!
//! // Events arrive one at a time ...
//! machine.record_point("temperature", TimePointEvent::new(10, 21.5_f64))?;
//! machine.record_span("up", TimeIntervalEvent::span(0, 10)?)?;
//!
//! // ... or a tick's worth at a time.
//! let mut tick = Batch::new();
//! tick.points("temperature", [TimePointEvent::new(11, 21.7_f64)])
//!     .points("log", [TimePointEvent::new(11, String::from("fan on"))])
//!     .spans("up", [TimeIntervalEvent::span(10, 20)?]);
//! machine.record_batch(tick)?;
//!
//! assert_eq!(machine.points::<f64>("temperature").map(|s| s.len()), Some(2));
//! assert_eq!(machine.intervals("up").map(|s| s.active_duration()), Some(20));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;

mod accessors;
mod batch;
mod erased;
mod errors;
mod mutation;
mod sequence;
mod traits;

pub use batch::Batch;
pub use erased::{AnyPointSequence, PointValue};
pub use errors::RecordError;
pub use sequence::Sequence;

/// A set of sequences, each corresponding to one attribute.
///
/// Members may be of either shape, and point sequences may each carry their
/// own value type; see [the module documentation](self) for the picture of a
/// sensor system this models.
///
/// # Attributes
///
/// A system keeps no attributes of its own: each sequence is filed under its
/// own [`attribute`], written out, so the attribute it is found by is always
/// the one it carries. A derived timeline is filed under its derivation,
/// `up ∪ busy` for instance, and a transformed one under `up[1, 2]`. Inserting
/// a sequence for an attribute already recorded replaces the old one and hands
/// it back, as `HashMap::insert` does.
///
/// That guarantee is why the system never lends out a sequence mutably: a
/// mutable sequence could be given another attribute while filed under its old
/// one. Events
/// go in through [`record_point`], [`record_span`] and [`record_batch`]
/// instead, and anything else — a change of attribute included — is a [`remove`] followed
/// by an [`insert`].
///
/// # Order and equality
///
/// Members are kept sorted by attribute, so iteration is in attribute order
/// however the system was built, and two systems are equal when they hold equal
/// sequences for the same attributes. Point sequences of different value types are never
/// equal, even when both are empty.
///
/// [`insert`]: SequenceSystem::insert
/// [`attribute`]: Sequence::attribute
/// [`record_batch`]: SequenceSystem::record_batch
/// [`record_point`]: SequenceSystem::record_point
/// [`record_span`]: SequenceSystem::record_span
/// [`remove`]: SequenceSystem::remove
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SequenceSystem {
    /// Keyed by each member's attribute, written out, which every method here
    /// keeps true: no member is reachable mutably except through recording, and
    /// recording never changes an attribute.
    sequences: BTreeMap<String, Sequence>,
}

impl SequenceSystem {
    /// Create a system with no sequences.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sequences: BTreeMap::new(),
        }
    }
}
