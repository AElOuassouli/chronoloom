//! The crate-private store of a stream system: erased sequences filed under
//! keys, with metadata, schema and extent.
//!
//! A [`Store`] files one sequence per [`Key`], of either shape, and point
//! sequences of any value type. It records events into a sequence by key and
//! cuts every member to an extent or a window of time. What a stream system
//! adds on top (frontiers, processors, drains) lives in `stream_system`; the
//! types a caller sees ([`Key`], [`Sequence`], [`AnyPointSequence`],
//! [`Metadata`], [`PointValue`], [`RecordError`]) are re-exported there.

use std::collections::BTreeMap;

use crate::values::{Schema, Value};

mod accessors;
mod drain;
mod erased;
mod errors;
mod key;
mod mutation;
mod sequence;
mod slice;

pub(crate) use erased::ErasedPoints;
pub use erased::{AnyPointSequence, AnyValue, PointValue};
pub use errors::RecordError;
pub use key::Key;
pub(crate) use mutation::Tail;
pub use sequence::Sequence;

/// Facts about a system or one of its members that chronoloom never
/// interprets: time unit, origin, sensor labels, anything the host needs to
/// carry along. Keys and values are plain strings, kept sorted by key.
pub type Metadata = BTreeMap<String, String>;

/// One member of a system: its sequence, and the metadata filed beside it.
#[derive(Debug, Clone)]
struct Member {
    sequence: Sequence,
    metadata: Metadata,
    /// Whether a processor wrote the member and nothing has been recorded into it since.
    derived: bool,
    /// For a member of a per-entity output, the entity it is the timeline of.
    entity: Option<Value>,
    schema: Option<Schema>,
}

impl PartialEq for Member {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
            && self.metadata == other.metadata
            && self.derived == other.derived
            && self.entity == other.entity
            && self.schema == other.schema
    }
}

/// A set of sequences, each filed under a [`Key`].
///
/// Members may be of either shape, and point sequences may each carry their
/// own value type. Every member is filed under a [`Key`], its identity: the key
/// is what a member is found, recorded into and removed by, and it never
/// changes while the member is in the store. Inserting a sequence under a key
/// already recorded replaces the old one and hands it back, as `HashMap::insert`
/// does. Members are kept sorted by key, and two stores are equal when they
/// hold equal members under equal keys.
///
/// The store itself carries [`Metadata`], and so does each member, as opaque
/// string maps nothing interprets. A member of record [`Value`]s may carry a
/// [`Schema`], checked when events are recorded, and a member written by a
/// processor is marked as derived until something is recorded into it.
///
/// [`Value`]: crate::Value
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Store {
    members: BTreeMap<Key, Member>,
    metadata: Metadata,
}

impl Store {
    /// Create a system with no sequences.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            members: BTreeMap::new(),
            metadata: Metadata::new(),
        }
    }
}
