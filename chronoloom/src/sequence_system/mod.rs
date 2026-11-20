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
//! use chronoloom::sequence_system::SequenceSystem;
//! use chronoloom::stream_system::Update;
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
//! let mut tick = Update::new();
//! tick.points("temperature", [TimePointEvent::new(11, 21.7_f64)])
//!     .points("log", [TimePointEvent::new(11, String::from("fan on"))])
//!     .spans("up", [TimeIntervalEvent::span(10, 20)?]);
//! machine.record_update(tick)?;
//!
//! assert_eq!(machine.points::<f64>("temperature").map(|s| s.len()), Some(2));
//! assert_eq!(machine.intervals("up").map(|s| s.active_duration()), Some(20));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;

use crate::primitives::Timestamp;
use crate::processing::Output;
use crate::values::{Schema, Value};

mod accessors;
mod drain;
mod erased;
mod errors;
mod key;
mod live;
mod mutation;
#[cfg(feature = "serde")]
mod parts;
mod sequence;
mod slice;
mod traits;

pub(crate) use erased::ErasedPoints;
pub use erased::{AnyPointSequence, AnyValue, PointValue};
pub use errors::RecordError;
pub use key::Key;
#[cfg(feature = "serde")]
pub(crate) use parts::MemberParts;
pub use sequence::Sequence;
pub use traits::{IntoIter, Iter};

/// Facts about a system or one of its members that chronoloom never
/// interprets: time unit, origin, sensor labels, anything the host needs to
/// carry along. Keys and values are plain strings, kept sorted by key.
pub type Metadata = BTreeMap<String, String>;

/// One member of a system: its sequence, and the metadata filed beside it.
#[derive(Debug, Clone)]
struct Member {
    sequence: Sequence,
    metadata: Metadata,
    frontier: Option<Timestamp>,
    lineage: Option<Output>,
    /// For a member of a per-entity output, the entity it is the timeline of.
    entity: Option<Value>,
    schema: Option<Schema>,
}

impl PartialEq for Member {
    /// The frontier is the state of a stream, not content: it is ignored.
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
            && self.metadata == other.metadata
            && self.lineage == other.lineage
            && self.entity == other.entity
            && self.schema == other.schema
    }
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
/// [`record_point`], [`record_span`] and [`record_update`], and anything else
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
/// [`record_update`] keep it. Metadata is part of equality, and [`clone`] and
/// [`slice`] carry it along; iterating by value yields the sequences only.
///
/// # Schema
///
/// A member whose events are record [`Value`]s may carry a [`Schema`], the
/// shape every value it holds must have. It belongs to the system, beside the
/// metadata and the lineage, and not to the sequence, which stays a plain
/// value. [`set_schema`] files it, checking the events already recorded and
/// refusing the schema if one does not have the shape, and [`schema`] reads
/// it. From then on [`record_point`] and [`record_update`] check every `Value`
/// event before recording it, and refuse one that does not have the shape with
/// [`RecordError::SchemaMismatch`], naming the path of the offending field; an
/// update is checked in its first pass with the rest, so it stays all or
/// nothing. A live member is checked like any other, and the check comes after
/// the one for lateness.
///
/// [`insert`] and [`insert_live`] start a member with no schema, and replacing
/// a member drops the old one's, as does [`remove`]. [`relabel`], [`slice`],
/// [`observe`], [`clone`] and [`drain_finalised`] keep it, and it is part of
/// equality. Only a member of `Value` points can have one: asking for another is
/// the same error as recording the wrong shape or value type.
///
/// # Slicing
///
/// [`slice`] cuts every member to a range of time, keeping each member's label,
/// its extent narrowed to the range, and all metadata. See
/// [`TimeIntervalSequence::slice`] for how a range maps onto the half-open
/// instants of a timeline.
///
/// # Live members and the frontier
///
/// A member is a batch member by default: it takes events in any order, and
/// its extent grows to cover them. [`insert_live`] files a live member
/// instead, for a stream. It carries a frontier, an instant before which its
/// data is final, and the frontier belongs to the system, beside the metadata,
/// not to the sequence: it is the state of a stream, not part of what a
/// sequence is. Equality ignores it, [`clone`] carries it, [`slice`] and
/// replacing the member by [`insert`] give a batch member, and
/// [`frontier_of`] and [`frontier`] (the earliest one) read it.
///
/// A live member refuses late events: a point before the frontier, or a span
/// starting before it, is [`RecordError::Late`], and an update holding one
/// records nothing. A point at the frontier is on time, and a span may reach
/// past it. The extent of a live member is what was observed, and only
/// [`advance`] observes: it moves the frontier forward and adds what it passed
/// over to the extent, since a live member watches continuously, while
/// [`advance_unobserved`] moves it without observing, leaving a gap for an
/// outage. [`advance_all`] advances every live member. Recording never grows
/// the extent past the frontier, so an event or the part of a span at or past
/// it is data outside the extent: it is not final yet, and it joins the
/// extent when the frontier passes it.
///
/// # Draining
///
/// A live member grows for as long as the stream runs, unless its final part is
/// handed over. [`drain_finalised`] takes it out of the system and returns it
/// as a system of its own, so memory stays bounded and the host decides what
/// becomes of it: store, publish or discard.
///
/// Each live member is drained by its own frontier, whatever its neighbours
/// do. A point sequence gives up the events before the frontier. A timeline
/// gives up the spans that end before it, whole: a span that reaches the
/// frontier stays, because it may still grow, even when it started long ago.
/// So what a live member holds is bounded by its open spans plus its
/// non-final events, the events at or past the frontier. The drained part
/// carries the extent before the earliest instant the member keeps (the start
/// of its first open span, or the frontier when there is none), and the member
/// keeps the extent from there on, so the two halves share what was observed
/// and neither holds data outside its own extent. Put back together, a drained
/// part and what remains are what a batch system of the same events would hold,
/// extent included. A span recorded after a drain starts at or past the
/// frontier, so it never touches a span already drained.
///
/// Every live member is in the drained system under its key, with its metadata
/// and the system's, even when its final part is empty: an empty part that
/// still has an extent says that nothing happened over that time, which is not
/// what no part says. An empty, unobserved part says nothing, and is as good as
/// absent. The drained members are batch members, since a stream that has
/// ended has no frontier. Batch members are not drained: they stay in the
/// system and are absent from the result.
///
/// [`drain_finalised`]: SequenceSystem::drain_finalised
/// [`schema`]: SequenceSystem::schema
/// [`set_schema`]: SequenceSystem::set_schema
/// [`Value`]: crate::Value
/// [`insert_live`]: SequenceSystem::insert_live
/// [`frontier_of`]: SequenceSystem::frontier_of
/// [`frontier`]: SequenceSystem::frontier
/// [`advance`]: SequenceSystem::advance
/// [`advance_unobserved`]: SequenceSystem::advance_unobserved
/// [`advance_all`]: SequenceSystem::advance_all
/// [`RecordError::Late`]: RecordError::Late
/// [`clone`]: Clone::clone
/// [`observe`]: SequenceSystem::observe
/// [`metadata`]: SequenceSystem::metadata
/// [`metadata_mut`]: SequenceSystem::metadata_mut
/// [`member_metadata`]: SequenceSystem::member_metadata
/// [`member_metadata_mut`]: SequenceSystem::member_metadata_mut
/// [`slice`]: SequenceSystem::slice
/// [`TimeIntervalSequence::slice`]: crate::sequences::TimeIntervalSequence::slice
/// [`relabel`]: SequenceSystem::relabel
/// [`attribute`]: Sequence::attribute
/// [`insert`]: SequenceSystem::insert
/// [`record_update`]: SequenceSystem::record_update
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
