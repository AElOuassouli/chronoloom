//! An always-ordered sequence of point events.

use std::ops::{Bound, Index, RangeBounds};
use std::{slice, vec};

use crate::primitives::{TimePointEvent, Timestamp};

/// A collection of [`TimePointEvent`]s that is always in time order.
///
/// Events may arrive in any order and several may share an instant; the
/// sequence always reads oldest first, and within one instant in the order the
/// events were added.
///
/// ```
/// use chronoloom::primitives::TimePointEvent;
/// use chronoloom::sequences::TimePointSequence;
///
/// let mut readings = TimePointSequence::new();
/// readings.insert(TimePointEvent::new(30, 'c'));
/// readings.insert(TimePointEvent::new(10, 'a'));
/// readings.insert(TimePointEvent::new(20, 'b'));
///
/// let order: Vec<char> = readings.iter().map(|e| *e.value()).collect();
/// assert_eq!(order, ['a', 'b', 'c']);
/// ```
///
/// # Cost
///
/// Events live in one contiguous `Vec`, kept sorted by timestamp. That layout
/// decides every cost here:
///
/// - Looking up an instant, a window, or a neighbour is logarithmic — a binary
///   search over the maintained order.
/// - Reading by position, or the sequence as a slice, is constant.
/// - [`insert`] is *amortized constant* when the event belongs at the end,
///   which is how events usually arrive. Inserting into the middle, or
///   [`remove`], costs a shift of everything after the touched instant. The
///   search stays logarithmic; the shift is a `memmove`, so it runs at memory
///   bandwidth rather than chasing pointers.
///
/// Building from an existing collection through [`FromIterator`] sorts once
/// rather than inserting one event at a time, so prefer `collect` to a loop of
/// [`insert`] when the events are already in hand.
///
/// [`insert`]: TimePointSequence::insert
/// [`remove`]: TimePointSequence::remove
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimePointSequence<T> {
    /// Sorted by timestamp, and stable within one instant: events sharing a
    /// timestamp stay in the order they were added. Every method here either
    /// preserves that or restores it before returning.
    events: Vec<TimePointEvent<T>>,
}

impl<T> TimePointSequence<T> {
    /// Create an empty sequence.
    ///
    /// ```
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<f64> = TimePointSequence::new();
    /// assert!(readings.is_empty());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// How many events the sequence holds.
    ///
    /// Counts events, not instants — several events may share a timestamp. See
    /// [`instant_count`] for the other number.
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
    ///
    /// [`instant_count`]: TimePointSequence::instant_count
    // Not a `const fn`: `Vec::len` is const-stable only from 1.87, past this
    // crate's 1.83 MSRV.
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
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(10, 'a'),
    ///     TimePointEvent::new(10, 'b'),
    ///     TimePointEvent::new(20, 'c'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings.instant_count(), 2);
    /// ```
    ///
    /// [`len`]: TimePointSequence::len
    #[must_use]
    pub fn instant_count(&self) -> usize {
        self.events
            .chunk_by(|a, b| a.timestamp() == b.timestamp())
            .count()
    }

    /// Whether the sequence holds no events at all.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// assert!(readings.is_empty());
    ///
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// assert!(!readings.is_empty());
    /// ```
    // Not a `const fn`, for the same reason as `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drop every event, leaving the sequence empty.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.clear();
    ///
    /// assert!(readings.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// The whole sequence as a slice, oldest event first.
    ///
    /// Constant time — the events already sit contiguously — so this is the
    /// way to hand a sequence to code that knows nothing about `chronoloom`.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(20, 'b'),
    ///     TimePointEvent::new(10, 'a'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// let pairs: Vec<(i64, char)> = readings
    ///     .as_slice()
    ///     .windows(2)
    ///     .map(|w| (w[1].timestamp() - w[0].timestamp(), *w[0].value()))
    ///     .collect();
    /// assert_eq!(pairs, [(10, 'a')]);
    /// ```
    #[must_use]
    pub fn as_slice(&self) -> &[TimePointEvent<T>] {
        &self.events
    }

    /// Add an event, keeping the sequence ordered.
    ///
    /// Never displaces anything: an instant that already holds events gains
    /// another, placed after them. Amortized constant when the event belongs
    /// at the end — the usual case for events arriving in time order —
    /// otherwise a logarithmic search followed by a shift.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.insert(TimePointEvent::new(10, 'b'));
    ///
    /// let values: Vec<char> = readings.get(10).iter().map(|e| *e.value()).collect();
    /// assert_eq!(values, ['a', 'b']);
    /// ```
    pub fn insert(&mut self, event: TimePointEvent<T>) {
        match self.events.last() {
            // Out of order, so pay for the search and the shift.
            Some(last) if last.timestamp() > event.timestamp() => {
                let index = self.upper_bound(event.timestamp());
                self.events.insert(index, event);
            }
            // At or after the end: it already belongs where it lands, after
            // any events sharing its instant.
            _ => self.events.push(event),
        }
    }

    /// Remove every event at `timestamp` and return them in order.
    ///
    /// Returns an empty `Vec` when the instant held nothing. Costs a
    /// logarithmic search plus a shift of everything after the instant.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.insert(TimePointEvent::new(10, 'b'));
    ///
    /// let removed = readings.remove(10);
    /// assert_eq!(removed.len(), 2);
    /// assert!(readings.remove(10).is_empty());
    /// assert!(readings.is_empty());
    /// ```
    pub fn remove(&mut self, timestamp: Timestamp) -> Vec<TimePointEvent<T>> {
        let start = self.lower_bound(timestamp);
        let end = self.upper_bound(timestamp);

        self.events.drain(start..end).collect()
    }

    /// The events recorded at `timestamp`, in the order they were added.
    ///
    /// Empty when nothing happened at that instant. They sit contiguously, so
    /// this is a slice of the sequence itself rather than a copy. Logarithmic.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    ///
    /// assert_eq!(readings.get(10).len(), 1);
    /// assert!(readings.get(20).is_empty());
    /// ```
    #[must_use]
    pub fn get(&self, timestamp: Timestamp) -> &[TimePointEvent<T>] {
        let start = self.lower_bound(timestamp);
        let end = self.upper_bound(timestamp);

        &self.events[start..end]
    }

    /// Whether any event was recorded at `timestamp`.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    ///
    /// assert!(readings.contains(10));
    /// assert!(!readings.contains(20));
    /// ```
    #[must_use]
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        !self.get(timestamp).is_empty()
    }

    /// The event at position `index`, counting from the oldest.
    ///
    /// Constant time. This asks a different question from [`get`], which looks
    /// up by instant rather than by position.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(20, 'b'),
    ///     TimePointEvent::new(10, 'a'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings.nth(0).map(|e| e.timestamp()), Some(10));
    /// assert_eq!(readings.nth(5), None);
    /// ```
    ///
    /// [`get`]: TimePointSequence::get
    #[must_use]
    pub fn nth(&self, index: usize) -> Option<&TimePointEvent<T>> {
        self.events.get(index)
    }

    /// Walk every event, oldest first.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(20, 'b'));
    /// readings.insert(TimePointEvent::new(10, 'a'));
    ///
    /// let seen: Vec<i64> = readings.iter().map(|e| e.timestamp()).collect();
    /// assert_eq!(seen, [10, 20]);
    /// ```
    pub fn iter(&self) -> slice::Iter<'_, TimePointEvent<T>> {
        self.events.iter()
    }

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

    /// The earliest event, or `None` when the sequence is empty.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(20, 'b'));
    /// readings.insert(TimePointEvent::new(10, 'a'));
    ///
    /// assert_eq!(readings.first().map(|e| e.timestamp()), Some(10));
    /// ```
    #[must_use]
    pub fn first(&self) -> Option<&TimePointEvent<T>> {
        self.events.first()
    }

    /// The latest event, or `None` when the sequence is empty.
    ///
    /// When several events share the latest instant, this is the last of them.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.insert(TimePointEvent::new(20, 'b'));
    ///
    /// assert_eq!(readings.last().map(|e| e.timestamp()), Some(20));
    /// ```
    #[must_use]
    pub fn last(&self) -> Option<&TimePointEvent<T>> {
        self.events.last()
    }

    /// The last event at or before `timestamp`.
    ///
    /// **The bound is inclusive**: an event landing exactly on `timestamp` is
    /// the answer. `None` when nothing happened that early. Logarithmic.
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
    /// assert_eq!(readings.before(15).map(|e| e.timestamp()), Some(10));
    /// assert_eq!(readings.before(20).map(|e| e.timestamp()), Some(20));
    /// assert_eq!(readings.before(5), None);
    /// ```
    #[must_use]
    pub fn before(&self, timestamp: Timestamp) -> Option<&TimePointEvent<T>> {
        let end = self.upper_bound(timestamp);

        self.events[..end].last()
    }

    /// The first event at or after `timestamp`.
    ///
    /// **The bound is inclusive**: an event landing exactly on `timestamp` is
    /// the answer. `None` when nothing happened that late. Logarithmic.
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
    /// assert_eq!(readings.after(15).map(|e| e.timestamp()), Some(20));
    /// assert_eq!(readings.after(10).map(|e| e.timestamp()), Some(10));
    /// assert_eq!(readings.after(25), None);
    /// ```
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
    fn lower_bound(&self, timestamp: Timestamp) -> usize {
        self.events
            .partition_point(|event| event.timestamp() < timestamp)
    }

    /// Index of the first event strictly after `timestamp`, so the end of the
    /// run of events sharing it.
    fn upper_bound(&self, timestamp: Timestamp) -> usize {
        self.events
            .partition_point(|event| event.timestamp() <= timestamp)
    }
}

impl<T> FromIterator<TimePointEvent<T>> for TimePointSequence<T> {
    /// Collect events into a sequence, in any order.
    ///
    /// Sorts once at the end rather than inserting one event at a time, so
    /// this is the cheap way to build a sequence from events already in hand.
    /// The sort is stable, so events sharing an instant keep their order.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(20, 'b'),
    ///     TimePointEvent::new(10, 'a'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings.first().map(|e| e.timestamp()), Some(10));
    /// ```
    fn from_iter<I: IntoIterator<Item = TimePointEvent<T>>>(events: I) -> Self {
        let mut events: Vec<TimePointEvent<T>> = events.into_iter().collect();
        events.sort_by_key(TimePointEvent::timestamp);

        Self { events }
    }
}

impl<T> Extend<TimePointEvent<T>> for TimePointSequence<T> {
    /// Add every event to the sequence, merging onto occupied instants.
    ///
    /// Appends first and reorders only if the additions actually broke the
    /// order, so extending with events that are already in time order costs no
    /// sorting at all.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.extend([TimePointEvent::new(10, 'b'), TimePointEvent::new(20, 'c')]);
    ///
    /// assert_eq!(readings.len(), 3);
    /// assert_eq!(readings.get(10).len(), 2);
    /// ```
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
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<char> = [
    ///     TimePointEvent::new(20, 'b'),
    ///     TimePointEvent::new(10, 'a'),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings[0].timestamp(), 10);
    /// ```
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
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(20, 'b'));
    /// readings.insert(TimePointEvent::new(10, 'a'));
    ///
    /// let owned: Vec<char> = readings.into_iter().map(TimePointEvent::into_value).collect();
    /// assert_eq!(owned, ['a', 'b']);
    /// ```
    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::TimePointSequence;
    use crate::primitives::TimePointEvent;
    use std::collections::BTreeSet;
    use std::ops::Bound;

    /// Build a sequence by inserting `(timestamp, value)` pairs one at a time,
    /// in the order given — the path `FromIterator` does not take.
    fn inserted<T>(events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
        let mut sequence = TimePointSequence::new();
        for (timestamp, value) in events {
            sequence.insert(TimePointEvent::new(timestamp, value));
        }

        sequence
    }

    /// Build a sequence from `(timestamp, value)` pairs through `collect`.
    fn collected<T>(events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
        events
            .into_iter()
            .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
            .collect()
    }

    /// The timestamps a sequence reads, in order.
    fn timestamps<T>(sequence: &TimePointSequence<T>) -> Vec<i64> {
        sequence.iter().map(TimePointEvent::timestamp).collect()
    }

    /// The values a sequence reads, in order.
    fn values<T: Copy>(sequence: &TimePointSequence<T>) -> Vec<T> {
        sequence.iter().map(|event| *event.value()).collect()
    }

    #[test]
    fn events_read_in_time_order_however_they_arrive() {
        assert_eq!(
            timestamps(&inserted([(30, 'c'), (10, 'a'), (20, 'b')])),
            [10, 20, 30]
        );
        assert_eq!(
            timestamps(&collected([(30, 'c'), (10, 'a'), (20, 'b')])),
            [10, 20, 30]
        );
    }

    #[test]
    fn insertion_and_collection_agree() {
        let events = [(30, 'c'), (10, 'a'), (20, 'b'), (10, 'z'), (-5, 'y')];

        assert_eq!(inserted(events), collected(events));
    }

    #[test]
    fn negative_timestamps_sort_before_positive_ones() {
        assert_eq!(
            timestamps(&inserted([(5, 'c'), (-10, 'a'), (0, 'b')])),
            [-10, 0, 5]
        );
    }

    #[test]
    fn several_events_may_share_an_instant() {
        let readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);

        assert_eq!(readings.len(), 3);
        assert_eq!(readings.instant_count(), 2);
        assert_eq!(readings.get(10).len(), 2);
        assert_eq!(timestamps(&readings), [10, 10, 20]);
    }

    #[test]
    fn events_sharing_an_instant_keep_insertion_order() {
        assert_eq!(
            values(&inserted([(10, 'c'), (10, 'a'), (10, 'b')])),
            ['c', 'a', 'b']
        );
    }

    #[test]
    fn an_out_of_order_insert_lands_after_events_sharing_its_instant() {
        // 'z' arrives last but belongs at instant 10, after 'a'.
        let readings = inserted([(10, 'a'), (30, 'c'), (10, 'z')]);

        assert_eq!(values(&readings), ['a', 'z', 'c']);
    }

    #[test]
    fn collecting_keeps_source_order_within_an_instant() {
        assert_eq!(
            values(&collected([(10, 'c'), (20, 'd'), (10, 'a')])),
            ['c', 'a', 'd']
        );
    }

    #[test]
    fn remove_takes_every_event_at_the_instant() {
        let mut readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);

        let removed: Vec<char> = readings
            .remove(10)
            .into_iter()
            .map(TimePointEvent::into_value)
            .collect();

        assert_eq!(removed, ['a', 'b']);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings.instant_count(), 1);
        assert!(!readings.contains(10));
    }

    #[test]
    fn remove_keeps_the_surrounding_events_in_order() {
        let mut readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);
        readings.remove(20);

        assert_eq!(timestamps(&readings), [10, 30]);
    }

    #[test]
    fn removing_an_absent_instant_changes_nothing() {
        let mut readings = inserted([(10, 'a')]);

        assert!(readings.remove(99).is_empty());
        assert!(readings.remove(-99).is_empty());
        assert_eq!(readings.len(), 1);
    }

    #[test]
    fn len_stays_exact_across_interleaved_mutations() {
        let mut readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);
        assert_eq!(readings.len(), 3);

        readings.remove(10);
        assert_eq!(readings.len(), 1);

        readings.extend([TimePointEvent::new(30, 'd'), TimePointEvent::new(30, 'e')]);
        assert_eq!(readings.len(), 3);

        readings.insert(TimePointEvent::new(20, 'f'));
        assert_eq!(readings.len(), 4);
        assert_eq!(timestamps(&readings), [20, 20, 30, 30]);

        readings.clear();
        assert_eq!(readings.len(), 0);
        assert_eq!(readings.instant_count(), 0);
        assert!(readings.is_empty());
    }

    #[test]
    fn len_counts_events_while_instant_count_counts_instants() {
        let readings = inserted([(10, 'a'), (10, 'b'), (10, 'c')]);

        assert_eq!(readings.len(), 3);
        assert_eq!(readings.instant_count(), 1);
    }

    #[test]
    fn get_is_empty_for_an_instant_that_holds_nothing() {
        let readings = inserted([(10, 'a'), (30, 'b')]);

        assert!(readings.get(20).is_empty());
        assert!(readings.get(0).is_empty());
        assert!(readings.get(99).is_empty());
        assert!(!readings.contains(20));
    }

    #[test]
    fn as_slice_exposes_every_event_in_order() {
        let readings = inserted([(20, 'b'), (10, 'a')]);

        assert_eq!(readings.as_slice().len(), 2);
        assert_eq!(readings.as_slice()[0].timestamp(), 10);
    }

    #[test]
    fn nth_and_indexing_read_by_position() {
        let readings = inserted([(20, 'b'), (10, 'a')]);

        assert_eq!(readings.nth(0).map(TimePointEvent::timestamp), Some(10));
        assert_eq!(readings.nth(1).map(TimePointEvent::timestamp), Some(20));
        assert!(readings.nth(2).is_none());
        assert_eq!(readings[1].timestamp(), 20);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn indexing_past_the_end_panics() {
        let readings = inserted([(10, 'a')]);

        let _ = readings[5];
    }

    #[test]
    fn range_excludes_its_end_and_includes_its_start() {
        let readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);

        let window: Vec<i64> = readings
            .range(10..30)
            .iter()
            .map(TimePointEvent::timestamp)
            .collect();
        assert_eq!(window, [10, 20]);
    }

    #[test]
    fn range_honours_inclusive_and_unbounded_ends() {
        let readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);

        assert_eq!(readings.range(10..=30).len(), 3);
        assert_eq!(readings.range(20..).len(), 2);
        assert_eq!(readings.range(..20).len(), 1);
        assert_eq!(readings.range(..).len(), 3);
    }

    #[test]
    fn range_between_occupied_instants_yields_nothing() {
        let readings = inserted([(10, 'a'), (30, 'b')]);

        assert!(readings.range(15..25).is_empty());
        assert!(readings.range(100..200).is_empty());
        assert!(readings.range(-200..-100).is_empty());
    }

    #[test]
    fn range_yields_every_event_at_a_shared_instant() {
        let readings = inserted([(10, 'a'), (20, 'b'), (20, 'c'), (30, 'd')]);

        let window: Vec<char> = readings.range(20..30).iter().map(|e| *e.value()).collect();
        assert_eq!(window, ['b', 'c']);
    }

    #[test]
    #[should_panic(expected = "range start is greater than range end")]
    fn an_inverted_range_panics() {
        let readings = inserted([(10, 'a'), (30, 'b')]);

        // Spelled with explicit bounds because clippy rejects the literal
        // `30..10` outright.
        let _ = readings.range((Bound::Included(30), Bound::Excluded(10)));
    }

    #[test]
    fn first_and_last_bracket_the_sequence() {
        let readings = inserted([(20, 'b'), (10, 'a'), (30, 'c')]);

        assert_eq!(readings.first().map(TimePointEvent::timestamp), Some(10));
        assert_eq!(readings.last().map(TimePointEvent::timestamp), Some(30));
    }

    #[test]
    fn first_and_last_pick_the_right_end_of_a_shared_instant() {
        let readings = inserted([(10, 'a'), (10, 'b')]);

        assert_eq!(readings.first().map(|e| *e.value()), Some('a'));
        assert_eq!(readings.last().map(|e| *e.value()), Some('b'));
    }

    #[test]
    fn before_and_after_include_an_exact_match() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        assert_eq!(readings.before(20).map(TimePointEvent::timestamp), Some(20));
        assert_eq!(readings.after(10).map(TimePointEvent::timestamp), Some(10));
    }

    #[test]
    fn before_and_after_step_to_the_neighbouring_instant() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        assert_eq!(readings.before(15).map(TimePointEvent::timestamp), Some(10));
        assert_eq!(readings.after(15).map(TimePointEvent::timestamp), Some(20));
    }

    #[test]
    fn before_and_after_pick_the_right_end_of_a_shared_instant() {
        let readings = inserted([(10, 'a'), (10, 'b')]);

        assert_eq!(readings.before(10).map(|e| *e.value()), Some('b'));
        assert_eq!(readings.after(10).map(|e| *e.value()), Some('a'));
    }

    #[test]
    fn before_and_after_run_out_past_the_edges() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        assert!(readings.before(5).is_none());
        assert!(readings.after(25).is_none());
    }

    #[test]
    fn nearest_picks_the_closer_side() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        assert_eq!(
            readings.nearest(12).map(TimePointEvent::timestamp),
            Some(10)
        );
        assert_eq!(
            readings.nearest(18).map(TimePointEvent::timestamp),
            Some(20)
        );
    }

    #[test]
    fn nearest_breaks_a_tie_toward_the_earlier_event() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        assert_eq!(
            readings.nearest(15).map(TimePointEvent::timestamp),
            Some(10)
        );
    }

    #[test]
    fn nearest_falls_back_to_whichever_side_exists() {
        let readings = inserted([(10, 'a')]);

        assert_eq!(readings.nearest(5).map(TimePointEvent::timestamp), Some(10));
        assert_eq!(
            readings.nearest(50).map(TimePointEvent::timestamp),
            Some(10)
        );
    }

    #[test]
    fn nearest_survives_the_timestamp_extremes() {
        let readings = inserted([(i64::MIN, 'a'), (i64::MAX, 'b')]);

        assert_eq!(
            readings.nearest(-1).map(TimePointEvent::timestamp),
            Some(i64::MIN)
        );
        assert_eq!(
            readings.nearest(1).map(TimePointEvent::timestamp),
            Some(i64::MAX)
        );
    }

    #[test]
    fn an_empty_sequence_answers_nothing() {
        let readings: TimePointSequence<char> = TimePointSequence::new();

        assert!(readings.is_empty());
        assert_eq!(readings.len(), 0);
        assert_eq!(readings.instant_count(), 0);
        assert!(readings.get(10).is_empty());
        assert!(readings.as_slice().is_empty());
        assert!(readings.range(..).is_empty());
        assert!(readings.nth(0).is_none());
        assert!(readings.first().is_none());
        assert!(readings.last().is_none());
        assert!(readings.before(10).is_none());
        assert!(readings.after(10).is_none());
        assert!(readings.nearest(10).is_none());
        assert_eq!(readings.iter().count(), 0);
    }

    #[test]
    fn new_and_default_agree() {
        let built: TimePointSequence<char> = TimePointSequence::new();

        assert_eq!(built, TimePointSequence::default());
    }

    #[test]
    fn collecting_round_trips_through_into_iter() {
        let readings = collected([(30, 'c'), (10, 'a'), (20, 'b'), (10, 'z')]);

        let owned: Vec<(i64, char)> = readings
            .clone()
            .into_iter()
            .map(TimePointEvent::into_parts)
            .collect();
        assert_eq!(owned, [(10, 'a'), (10, 'z'), (20, 'b'), (30, 'c')]);

        let rebuilt: TimePointSequence<char> = owned
            .into_iter()
            .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
            .collect();
        assert_eq!(rebuilt, readings);
    }

    #[test]
    fn extend_merges_onto_occupied_instants() {
        let mut readings = inserted([(10, 'a')]);
        readings.extend([TimePointEvent::new(10, 'b'), TimePointEvent::new(20, 'c')]);

        assert_eq!(readings.len(), 3);
        assert_eq!(values(&readings), ['a', 'b', 'c']);
    }

    #[test]
    fn extend_reorders_when_the_additions_arrive_out_of_order() {
        let mut readings = inserted([(20, 'b')]);
        readings.extend([TimePointEvent::new(30, 'c'), TimePointEvent::new(10, 'a')]);

        assert_eq!(timestamps(&readings), [10, 20, 30]);
    }

    #[test]
    fn extend_onto_an_empty_sequence_still_orders() {
        let mut readings = TimePointSequence::new();
        readings.extend([TimePointEvent::new(20, 'b'), TimePointEvent::new(10, 'a')]);

        assert_eq!(timestamps(&readings), [10, 20]);
    }

    #[test]
    fn borrowed_iteration_leaves_the_sequence_usable() {
        let readings = inserted([(10, 'a'), (20, 'b')]);

        let borrowed: Vec<i64> = (&readings)
            .into_iter()
            .map(TimePointEvent::timestamp)
            .collect();

        assert_eq!(borrowed, [10, 20]);
        assert_eq!(readings.len(), 2);
    }

    #[test]
    fn payload_may_be_a_non_copy_value() {
        let readings = inserted([(10, String::from("start")), (20, String::from("stop"))]);

        assert_eq!(readings.first().map(|e| e.value().as_str()), Some("start"));
        assert_eq!(readings.get(20)[0].value(), "stop");
    }

    #[test]
    fn payload_may_be_a_collection() {
        let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
        let readings = inserted([(10, tags.clone())]);

        assert_eq!(readings.get(10)[0].value(), &tags);
    }
}
