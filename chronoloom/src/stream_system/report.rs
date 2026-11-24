//! What a change to a stream system did.

use std::collections::{BTreeMap, BTreeSet};

use crate::operator::Chunk;
use crate::primitives::Timestamp;
use crate::store::Key;

/// What a change to a [`StreamSystem`] did to its derived streams.
///
/// An [`update`](super::StreamSystem::update), `insert_stream` or
/// `add_processor` of a batch system evaluates again only the processors that
/// read what changed. A live system returns a [`LiveReport`] instead.
/// `changed` names the derived streams whose content differs afterwards, and
/// `evaluated` the outputs of every processor that ran, whether or not its
/// output differs, so a host can tell "unchanged" from "not looked at".
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub(super) changed: BTreeSet<Key>,
    pub(super) evaluated: BTreeSet<Key>,
}

impl Report {
    /// The derived streams whose content changed, appeared or disappeared.
    #[must_use]
    pub const fn changed(&self) -> &BTreeSet<Key> {
        &self.changed
    }

    /// The outputs of the processors that were evaluated again, whether or
    /// not their content changed.
    #[must_use]
    pub const fn evaluated(&self) -> &BTreeSet<Key> {
        &self.evaluated
    }

    /// Whether no derived stream changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }
}

/// What an update of a live [`StreamSystem`] did: what became final in each
/// derived stream, and how far each derived stream is final.
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveReport {
    pub(super) chunks: BTreeMap<Key, Chunk>,
    pub(super) frontiers: BTreeMap<Key, Timestamp>,
}

impl LiveReport {
    /// For each derived stream that made progress, the [`Chunk`] that became
    /// final: its sequence over `[old frontier, new frontier)`. Appended to
    /// what the stream held, it is the stream. A member of a per-entity
    /// output seen for the first time has a chunk that starts at the start of
    /// time.
    #[must_use]
    pub const fn chunks(&self) -> &BTreeMap<Key, Chunk> {
        &self.chunks
    }

    /// The frontier of every derived stream after the update, progress or not:
    /// everything before it is final.
    #[must_use]
    pub const fn frontiers(&self) -> &BTreeMap<Key, Timestamp> {
        &self.frontiers
    }

    /// Whether no derived stream made progress.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}
