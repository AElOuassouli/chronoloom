//! The standard-library traits a timeline implements.

use core::fmt;
use std::ops::Index;
use std::{slice, vec};

use crate::primitives::TimeIntervalEvent;

use super::normalization::normalize;
use super::TimeIntervalSequence;

impl fmt::Display for TimeIntervalSequence {
    /// How the timeline describes itself: its attribute, with its
    /// transformation applied.
    ///
    /// The spans are not written — this says *what* the timeline is, not what
    /// is in it. Use [`Debug`] for that.
    ///
    /// [`Debug`]: core::fmt::Debug
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Built rather than matched on, so the grouping rules live in exactly
        // one place — `Attribute`'s own `Display`.
        write!(f, "{}", self.folded_attribute())
    }
}

impl PartialEq for TimeIntervalSequence {
    /// Two timelines are equal when they describe the same thing and cover the
    /// same instants.
    ///
    /// The description counts because it is part of what a timeline *is*: `up`
    /// and `reachable` are two states, even on a day they happened to coincide.
    /// [`covers_same`] is the other half on its own, and the one the set-algebra
    /// identities are stated with.
    ///
    /// Written out rather than derived so the spans and the label alone decide.
    /// The duration is a function of the spans — comparing it could only ever
    /// agree, or reveal a bookkeeping bug by disagreeing, and equality is the
    /// wrong place to discover that.
    ///
    /// [`covers_same`]: TimeIntervalSequence::covers_same
    fn eq(&self, other: &Self) -> bool {
        self.spans == other.spans
            && self.attribute == other.attribute
            && self.transformation == other.transformation
    }
}

impl Eq for TimeIntervalSequence {}

impl Extend<TimeIntervalEvent<()>> for TimeIntervalSequence {
    /// Mark every span as covered, merging into what is already there.
    fn extend<I: IntoIterator<Item = TimeIntervalEvent<()>>>(&mut self, spans: I) {
        // Appending and renormalizing beats inserting one at a time: it shifts
        // the tail once rather than once per span.
        let mut combined = std::mem::take(&mut self.spans);
        combined.extend(spans);

        // Marking more of a state covered does not change which state it is, so
        // the description carries over untouched.
        *self = Self::from_parts(
            self.attribute.clone(),
            self.transformation,
            normalize(combined),
        );
    }
}

impl Index<usize> for TimeIntervalSequence {
    type Output = TimeIntervalEvent<()>;

    /// The span at position `index`, counting from the earliest.
    ///
    /// # Panics
    ///
    /// If `index` is past the end. Use [`nth`] to get an `Option` instead.
    ///
    /// [`nth`]: TimeIntervalSequence::nth
    fn index(&self, index: usize) -> &Self::Output {
        &self.spans[index]
    }
}

impl<'a> IntoIterator for &'a TimeIntervalSequence {
    type Item = &'a TimeIntervalEvent<()>;
    type IntoIter = slice::Iter<'a, TimeIntervalEvent<()>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for TimeIntervalSequence {
    type Item = TimeIntervalEvent<()>;
    type IntoIter = vec::IntoIter<TimeIntervalEvent<()>>;

    /// Consume the sequence, yielding owned spans earliest first.
    fn into_iter(self) -> Self::IntoIter {
        self.spans.into_iter()
    }
}
