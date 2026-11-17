//! Cutting a sequence to a range of time.

use std::ops::{Bound, RangeBounds};

use crate::primitives::Timestamp;
use crate::sequences::extent::Extent;

use super::TimePointSequence;

impl<T: Clone> TimePointSequence<T> {
    /// The part of the sequence inside `range`: its events there, its extent
    /// narrowed to the range, and the same label.
    ///
    /// Accepts any Rust range. Unlike [`range`], a reversed range is not an
    /// error: it holds no instant, so the result is empty and observed
    /// nowhere. The instant `i64::MAX` is outside every extent, so an inclusive
    /// end of `i64::MAX` reaches as far as an unbounded one, and an event
    /// stamped there is kept exactly when the range reaches it.
    ///
    /// [`range`]: TimePointSequence::range
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<Timestamp>) -> Self {
        let start = match range.start_bound() {
            Bound::Unbounded => 0,
            Bound::Included(&timestamp) => self.lower_bound(timestamp),
            Bound::Excluded(&timestamp) => self.upper_bound(timestamp),
        };
        let end = match range.end_bound() {
            Bound::Unbounded => self.events.len(),
            Bound::Included(&timestamp) => self.upper_bound(timestamp),
            Bound::Excluded(&timestamp) => self.lower_bound(timestamp),
        };
        let events = if start < end {
            self.events[start..end].to_vec()
        } else {
            Vec::new()
        };

        let mut observed = self.observed.clone();
        observed.narrow(&Extent::of_range(&range));

        Self {
            events,
            attribute: self.attribute.clone(),
            observed,
        }
    }
}
