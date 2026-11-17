//! Reading a system: which keys it records, and the sequence for each.

use crate::sequences::{TimeIntervalSequence, TimePointSequence};

use super::{Iter, Key, Metadata, Sequence, SequenceSystem};

impl SequenceSystem {
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
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    /// use chronoloom::sequence_system::SequenceSystem;
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert("temperature", TimePointSequence::from_events("temperature", vec![
    ///     TimePointEvent::new(10, 21.5_f64),
    /// ]));
    ///
    /// assert!(system.points::<f64>("temperature").is_some());
    /// assert!(system.points::<String>("temperature").is_none());
    /// assert!(system.points::<f64>("humidity").is_none());
    /// ```
    ///
    /// [`get`]: SequenceSystem::get
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

    /// Walk every sequence with its key, in key order.
    pub fn iter(&self) -> Iter<'_> {
        Iter(self.members.iter())
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
