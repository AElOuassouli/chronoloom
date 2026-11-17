//! Systems of sequences: everything recorded about one subject, side by side.
//!
//! A [`SequenceSystem`] is to sequences what a sensor system is to sensors. A
//! machine has a thermometer reporting `f64` readings, a logger reporting
//! `String` lines, and an uptime state that is either on or off — three
//! sequences of two shapes and three value types, describing one subject. The
//! system records one sequence per key, and records events into each as
//! the events arrive.
//!
//! ```
//! use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
//! use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};
//! use chronoloom::sequence_system::{Batch, SequenceSystem};
//!
//! // Declare each sensor once, under the key that identifies it.
//! let mut machine = SequenceSystem::new();
//! machine.insert("temperature", TimePointSequence::<f64>::new("temperature"));
//! machine.insert("log", TimePointSequence::<String>::new("log"));
//! machine.insert("up", TimeIntervalSequence::new("up"));
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
mod key;
mod mutation;
mod sequence;
mod slice;
mod traits;

pub use batch::Batch;
pub use erased::{AnyPointSequence, PointValue};
pub use errors::RecordError;
pub use key::Key;
pub use sequence::Sequence;
pub use traits::{IntoIter, Iter};

/// Facts about a system or one of its members that chronoloom never
/// interprets: time unit, origin, sensor labels, anything the host needs to
/// carry along. Keys and values are plain strings, kept sorted by key.
pub type Metadata = BTreeMap<String, String>;

/// One member of a system: its sequence, and the metadata filed beside it.
#[derive(Debug, Clone, PartialEq)]
struct Member {
    sequence: Sequence,
    metadata: Metadata,
}

/// A set of sequences, each filed under a [`Key`].
///
/// Members may be of either shape, and point sequences may each carry their
/// own value type; see [the module documentation](self) for the picture of a
/// sensor system this models.
///
/// # Keys and attributes
///
/// Every member is filed under a [`Key`], its identity: the key is what a
/// member is found, recorded into and removed by, and it never changes while
/// the member is in the system. The member's own [`attribute`] stays what it
/// always was, a description of what the sequence records. Several members may
/// share one attribute under different keys, and a member's attribute may
/// change through [`relabel`] without disturbing its key or its data.
/// Inserting a sequence under a key already recorded replaces the old one and
/// hands it back, as `HashMap::insert` does.
///
/// Mutable access to a member is not offered. Events go in through
/// [`record_point`], [`record_span`] and [`record_batch`], and anything else
/// is a [`remove`] followed by an [`insert`].
///
/// # Order and equality
///
/// Members are kept sorted by key, so iteration is in key order however the
/// system was built, and two systems are equal when they hold equal sequences
/// under equal keys. The attribute is part of each sequence's equality. Point
/// sequences of different value types are never equal, even when both are
/// empty.
///
/// # Metadata
///
/// The system itself carries [`Metadata`], and so does each member, as opaque
/// string maps chronoloom never interprets: a time unit, an origin, the
/// host's own labels. They are held here rather than on the sequences, so a
/// sequence stays a pure value and no operation has to decide which metadata a
/// result inherits. Read them with [`metadata`] and [`member_metadata`], and
/// change them in place through [`metadata_mut`] and [`member_metadata_mut`];
/// lending them mutably is safe because metadata never affects how a member is
/// filed.
///
/// [`insert`] starts a member with empty metadata, and replacing a member drops
/// the metadata of the old one, as does [`remove`]. [`relabel`] and
/// [`record_batch`] keep it. Metadata is part of equality, and [`clone`] and
/// [`slice`] carry it along; iterating by value yields the sequences only.
///
/// # Slicing
///
/// [`slice`] cuts every member to a range of time, keeping each member's label,
/// its extent narrowed to the range, and all metadata. See
/// [`TimeIntervalSequence::slice`] for how a range maps onto the half-open
/// instants of a timeline.
///
/// [`clone`]: Clone::clone
/// [`metadata`]: SequenceSystem::metadata
/// [`metadata_mut`]: SequenceSystem::metadata_mut
/// [`member_metadata`]: SequenceSystem::member_metadata
/// [`member_metadata_mut`]: SequenceSystem::member_metadata_mut
/// [`slice`]: SequenceSystem::slice
/// [`TimeIntervalSequence::slice`]: crate::sequences::TimeIntervalSequence::slice
/// [`relabel`]: SequenceSystem::relabel
/// [`attribute`]: Sequence::attribute
/// [`insert`]: SequenceSystem::insert
/// [`record_batch`]: SequenceSystem::record_batch
/// [`record_point`]: SequenceSystem::record_point
/// [`record_span`]: SequenceSystem::record_span
/// [`remove`]: SequenceSystem::remove
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SequenceSystem {
    members: BTreeMap<Key, Member>,
    metadata: Metadata,
}

impl SequenceSystem {
    /// Create a system with no sequences.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            members: BTreeMap::new(),
            metadata: Metadata::new(),
        }
    }
}
