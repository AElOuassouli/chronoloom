//! Windows and neighbours: the questions answered by binary-searching the
//! maintained order rather than scanning it.

use std::ops::{Bound, RangeBounds};

use crate::primitives::{TimePointEvent, Timestamp};

use super::TimePointSequence;

impl<T> TimePointSequence<T> {
    /// The events whose timestamps fall inside `range`, oldest first.
    ///
    /// Accepts any Rust range, so the bounds may be inclusive, exclusive, or
    /// absent. Locating the window is logarithmic, and the window is returned
    /// as a slice of the sequence rather than copied out.
    ///
    /// # Panics
    ///
    /// If the range's start falls after its end, matching `BTreeMap::range`.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(10, 'a'),
    ///     TimePointEvent::new(20, 'b'),
    ///     TimePointEvent::new(30, 'c'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// let window: Vec<i64> = readings.range(10..30).iter().map(|e| e.timestamp()).collect();
    /// assert_eq!(window, [10, 20]);
    ///
    /// let inclusive: Vec<i64> = readings.range(10..=30).iter().map(|e| e.timestamp()).collect();
    /// assert_eq!(inclusive, [10, 20, 30]);
    /// ```
    #[must_use]
    pub fn range<R>(&self, range: R) -> &[TimePointEvent<T>]
    where
        R: RangeBounds<Timestamp>,
    {
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

        assert!(start <= end, "range start is greater than range end");

        &self.events[start..end]
    }

    /// The last event at or before `timestamp`.
    ///
    /// **The bound is inclusive**: an event landing exactly on `timestamp` is
    /// the answer. `None` when nothing happened that early. Logarithmic.
    #[must_use]
    pub fn before(&self, timestamp: Timestamp) -> Option<&TimePointEvent<T>> {
        let end = self.upper_bound(timestamp);

        self.events[..end].last()
    }

    /// The first event at or after `timestamp`.
    ///
    /// **The bound is inclusive**: an event landing exactly on `timestamp` is
    /// the answer. `None` when nothing happened that late. Logarithmic.
    #[must_use]
    pub fn after(&self, timestamp: Timestamp) -> Option<&TimePointEvent<T>> {
        let start = self.lower_bound(timestamp);

        self.events[start..].first()
    }

    /// The event closest in time to `timestamp`, in either direction.
    ///
    /// An exact match wins outright. A tie between one event before and one
    /// after goes to the earlier. `None` only when the sequence is empty.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(10, 'a'),
    ///     TimePointEvent::new(20, 'b'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings.nearest(12).map(|e| e.timestamp()), Some(10));
    /// assert_eq!(readings.nearest(18).map(|e| e.timestamp()), Some(20));
    /// // Exactly between the two: the earlier one wins.
    /// assert_eq!(readings.nearest(15).map(|e| e.timestamp()), Some(10));
    /// ```
    #[must_use]
    pub fn nearest(&self, timestamp: Timestamp) -> Option<&TimePointEvent<T>> {
        match (self.before(timestamp), self.after(timestamp)) {
            (Some(earlier), Some(later)) => {
                // `abs_diff` yields a u64, so even i64::MIN against i64::MAX
                // cannot overflow the way a subtraction would.
                if later.timestamp().abs_diff(timestamp) < earlier.timestamp().abs_diff(timestamp) {
                    Some(later)
                } else {
                    Some(earlier)
                }
            }
            (earlier, later) => earlier.or(later),
        }
    }

    /// Index of the first event at or after `timestamp`.
    ///
    /// `partition_point` rather than `binary_search_by_key`, which would
    /// return an arbitrary member of a run of equal timestamps rather than its
    /// first.
    pub(super) fn lower_bound(&self, timestamp: Timestamp) -> usize {
        self.events
            .partition_point(|event| event.timestamp() < timestamp)
    }

    /// Index of the first event strictly after `timestamp`, so the end of the
    /// run of events sharing it.
    pub(super) fn upper_bound(&self, timestamp: Timestamp) -> usize {
        self.events
            .partition_point(|event| event.timestamp() <= timestamp)
    }
}
