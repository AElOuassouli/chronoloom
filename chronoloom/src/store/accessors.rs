//! Reading a system: which keys it records, and the sequence for each.

use crate::sequences::{TimeIntervalSequence, TimePointSequence};
use crate::values::{Schema, Value};

use super::{Key, Metadata, Sequence, Store};

impl Store {
    /// How many sequences the system holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the system holds no sequences at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Whether the system records a sequence for `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.members.contains_key(key)
    }

    /// Whether a processor wrote the member under `key` and nothing has been
    /// recorded into it since.
    #[must_use]
    pub(crate) fn is_derived(&self, key: &str) -> bool {
        self.members.get(key).is_some_and(|member| member.derived)
    }

    /// The entity the member under `key` is the timeline of, if it was written
    /// by a per-entity output and nothing has been recorded
    /// into it since.
    #[must_use]
    pub(crate) fn derived_entity(&self, key: &str) -> Option<&Value> {
        self.members
            .get(key)
            .and_then(|member| member.entity.as_ref())
    }

    /// The sequence for `key`, of whichever shape.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Sequence> {
        self.members.get(key).map(|member| &member.sequence)
    }

    /// The point sequence for `key`, if there is one and its events carry
    /// values of type `T`.
    ///
    /// `None` does not say which of those failed; [`get`] does.
    ///
    /// [`get`]: Store::get
    #[must_use]
    pub fn points<T: 'static>(&self, key: &str) -> Option<&TimePointSequence<T>> {
        self.get(key)?.as_time_point()
    }

    /// The timeline for `key`, if there is one.
    #[must_use]
    pub fn intervals(&self, key: &str) -> Option<&TimeIntervalSequence> {
        self.get(key)?.as_time_interval()
    }

    /// Every key the system records, in order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &Key> + ExactSizeIterator {
        self.members.keys()
    }

    /// The schema of the member for `key`, if the system records one and a
    /// schema was set for it.
    #[must_use]
    pub fn schema(&self, key: &str) -> Option<&Schema> {
        self.members.get(key)?.schema.as_ref()
    }

    /// The metadata of the system as a whole.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The metadata of the system as a whole, for changing in place.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// The metadata of the member for `key`, if the system records one.
    #[must_use]
    pub fn member_metadata(&self, key: &str) -> Option<&Metadata> {
        self.members.get(key).map(|member| &member.metadata)
    }

    /// The metadata of the member for `key`, for changing in place.
    pub fn member_metadata_mut(&mut self, key: &str) -> Option<&mut Metadata> {
        self.members.get_mut(key).map(|member| &mut member.metadata)
    }
}
