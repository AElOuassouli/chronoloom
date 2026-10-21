//! Reading a sequence: how much it holds, and what sits at an instant or a
//! position.

use std::slice;

use crate::primitives::{TimePointEvent, Timestamp};

use super::TimePointSequence;

impl<T> TimePointSequence<T> {
    /// Consume the sequence and return its events, oldest first.
    ///
    /// The inverse of [`from_events`], and free: the sequence hands over the
    /// `Vec` it was already holding.
    ///
    /// [`from_events`]: TimePointSequence::from_events
    #[must_use]
    pub fn into_events(self) -> Vec<TimePointEvent<T>> {
        self.events
    }

    /// How many events the sequence holds.
    ///
    /// Counts events, not instants — several events may share a timestamp.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.insert(TimePointEvent::new(10, 'b'));
    ///
    /// assert_eq!(readings.len(), 2);
    /// assert_eq!(readings.instant_count(), 1);
    /// ```
    // Not a `const fn`: `Vec::len` is const-stable only from 1.87, past this
    // crate's 1.83 minimum supported Rust version.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// How many distinct instants the sequence covers.
    ///
    /// Equals [`len`] only when no two events share a timestamp. Unlike the
    /// rest of this type, counting instants walks the whole sequence — the
    /// contiguous layout stores no instant index to consult.
    ///
    /// [`len`]: TimePointSequence::len
    #[must_use]
    pub fn instant_count(&self) -> usize {
        self.events
            .chunk_by(|a, b| a.timestamp() == b.timestamp())
            .count()
    }

    /// Whether the sequence holds no events at all.
    // Not a `const fn`, for the same reason as `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The whole sequence as a slice, oldest event first.
    ///
    /// Constant time — the events already sit contiguously — so this is the way
    /// to hand a sequence to code that knows nothing about `chronoloom`.
    #[must_use]
    pub fn as_slice(&self) -> &[TimePointEvent<T>] {
        &self.events
    }

    /// The events recorded at `timestamp`, in the order they were added.
    ///
    /// Empty when nothing happened at that instant. They sit contiguously, so
    /// this is a slice of the sequence itself rather than a copy. Logarithmic.
    #[must_use]
    pub fn get(&self, timestamp: Timestamp) -> &[TimePointEvent<T>] {
        let start = self.lower_bound(timestamp);
        let end = self.upper_bound(timestamp);

        &self.events[start..end]
    }

    /// Whether any event was recorded at `timestamp`.
    #[must_use]
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        !self.get(timestamp).is_empty()
    }

    /// The event at position `index`, counting from the oldest.
    ///
    /// Constant time. This asks a different question from [`get`], which looks
    /// up by instant rather than by position.
    ///
    /// [`get`]: TimePointSequence::get
    #[must_use]
    pub fn nth(&self, index: usize) -> Option<&TimePointEvent<T>> {
        self.events.get(index)
    }

    /// Walk every event, oldest first.
    pub fn iter(&self) -> slice::Iter<'_, TimePointEvent<T>> {
        self.events.iter()
    }

    /// The earliest event, or `None` when the sequence is empty.
    ///
    /// When several events share the earliest instant, this is the first of
    /// them.
    #[must_use]
    pub fn first(&self) -> Option<&TimePointEvent<T>> {
        self.events.first()
    }

    /// The latest event, or `None` when the sequence is empty.
    ///
    /// When several events share the latest instant, this is the last of them.
    #[must_use]
    pub fn last(&self) -> Option<&TimePointEvent<T>> {
        self.events.last()
    }
}
