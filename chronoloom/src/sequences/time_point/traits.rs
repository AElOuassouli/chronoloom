//! The standard-library traits a point sequence implements.

use core::fmt;
use std::ops::Index;
use std::{slice, vec};

use crate::primitives::TimePointEvent;

use super::TimePointSequence;

impl<T> fmt::Display for TimePointSequence<T> {
    /// How the sequence describes itself: what it is a record of.
    ///
    /// The events are not written — this says *what* the sequence is, not what
    /// is in it. Use [`Debug`] for that.
    ///
    /// [`Debug`]: core::fmt::Debug
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.attribute)
    }
}

impl<T> Extend<TimePointEvent<T>> for TimePointSequence<T> {
    /// Add every event to the sequence, merging onto occupied instants.
    ///
    /// Appends first and reorders only if the additions actually broke the
    /// order, so extending with events that are already in time order costs no
    /// sorting at all.
    fn extend<I: IntoIterator<Item = TimePointEvent<T>>>(&mut self, events: I) {
        // Where the old events end, stepped back one so the check covers the
        // seam between what was already there and what is being added.
        let seam = self.events.len().saturating_sub(1);
        self.events.extend(events);

        if !self.events[seam..].is_sorted_by_key(TimePointEvent::timestamp) {
            self.events.sort_by_key(TimePointEvent::timestamp);
        }
    }
}

impl<T> Index<usize> for TimePointSequence<T> {
    type Output = TimePointEvent<T>;

    /// The event at position `index`, counting from the oldest.
    ///
    /// # Panics
    ///
    /// If `index` is past the end. Use [`nth`] to get an `Option` instead.
    ///
    /// [`nth`]: TimePointSequence::nth
    fn index(&self, index: usize) -> &Self::Output {
        &self.events[index]
    }
}

impl<'a, T> IntoIterator for &'a TimePointSequence<T> {
    type Item = &'a TimePointEvent<T>;
    type IntoIter = slice::Iter<'a, TimePointEvent<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> IntoIterator for TimePointSequence<T> {
    type Item = TimePointEvent<T>;
    type IntoIter = vec::IntoIter<TimePointEvent<T>>;

    /// Consume the sequence, yielding owned events oldest first.
    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}
