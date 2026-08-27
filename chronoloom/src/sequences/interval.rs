//! A normalized timeline of the spans during which one state was active.

use std::ops::Index;
use std::{slice, vec};

use crate::primitives::{TimeIntervalEvent, Timestamp};

/// The spans during which one state was active, in canonical form.
///
/// Every span in a sequence means the same thing, so the sequence describes a
/// single state over time rather than a collection of unrelated intervals. It
/// is kept **normalized**: sorted by start, pairwise disjoint, and with no two
/// spans left touching. Overlapping and touching spans are merged as they
/// arrive, since `[0, 5)` and `[5, 9)` together cover exactly `[0, 9)` — the
/// same rule [`TimeIntervalEvent::union`] applies to a pair.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let mut uptime = TimeIntervalSequence::new();
/// uptime.insert(TimeIntervalEvent::span(0, 10)?);
/// uptime.insert(TimeIntervalEvent::span(5, 20)?);
///
/// // The two overlapped, so they are one span now.
/// assert_eq!(uptime.len(), 1);
/// assert_eq!(uptime[0].bounds(), (0, 20));
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// # Coverage, not history
///
/// Normalization means a sequence records *which instants are covered*, not
/// which spans were inserted. Two sequences are equal exactly when they cover
/// the same instants, however they were built, and [`len`] counts the spans
/// that remain after merging rather than the number inserted.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let piecemeal = TimeIntervalSequence::from_spans(vec![
///     TimeIntervalEvent::span(0, 5)?,
///     TimeIntervalEvent::span(5, 10)?,
///     TimeIntervalEvent::span(2, 7)?,
/// ]);
/// let whole = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 10)?]);
///
/// assert_eq!(piecemeal, whole);
/// assert_eq!(piecemeal.len(), 1);
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// # Values
///
/// A sequence carries no payload. Because every span means the same state, a
/// value would belong to the sequence as a whole rather than to each span, and
/// state values are not modelled yet.
///
/// [`len`]: TimeIntervalSequence::len
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimeIntervalSequence {
    /// Normalized: sorted by start, pairwise disjoint, and never touching — so
    /// `spans[i].end() < spans[i + 1].start()` strictly, for every adjacent
    /// pair. Every method restores this before returning.
    spans: Vec<TimeIntervalEvent<()>>,
}

impl TimeIntervalSequence {
    /// Create an empty sequence, covering no instants at all.
    ///
    /// ```
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::new();
    /// assert!(uptime.is_empty());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Build a sequence from spans already in hand.
    ///
    /// The usual way to create one when the data exists up front. The spans may
    /// be in any order and may overlap freely; they are sorted once, in place,
    /// then merged into canonical form in a single pass.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    ///     TimeIntervalEvent::span(5, 20)?,
    /// ]);
    ///
    /// let bounds: Vec<(i64, i64)> = uptime.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 20), (30, 40)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn from_spans(mut spans: Vec<TimeIntervalEvent<()>>) -> Self {
        spans.sort_by_key(TimeIntervalEvent::start);

        // Sweep once, folding each span into the one being built whenever they
        // overlap or touch. The sort guarantees every span that can merge with
        // the current one comes next, so a single pass is enough.
        let mut normalized: Vec<TimeIntervalEvent<()>> = Vec::with_capacity(spans.len());
        for span in spans {
            match normalized.pop() {
                // Overlapping or touching the span still being built: widen it.
                Some(open) if span.start() <= open.end() => normalized.push(
                    TimeIntervalEvent::raw(open.start(), open.end().max(span.end())),
                ),
                // A real gap, so the previous span is finished.
                Some(open) => {
                    normalized.push(open);
                    normalized.push(span);
                }
                None => normalized.push(span),
            }
        }

        Self { spans: normalized }
    }

    /// Consume the sequence and return its spans, earliest first.
    ///
    /// The inverse of [`from_spans`], and free: the sequence hands over the
    /// `Vec` it was already holding. These are the normalized spans, not
    /// whatever was originally inserted.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(0, 10)?,
    ///     TimeIntervalEvent::span(5, 20)?,
    /// ]);
    ///
    /// let spans = uptime.into_spans();
    /// assert_eq!(spans.len(), 1);
    /// assert_eq!(spans[0].bounds(), (0, 20));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`from_spans`]: TimeIntervalSequence::from_spans
    #[must_use]
    pub fn into_spans(self) -> Vec<TimeIntervalEvent<()>> {
        self.spans
    }

    /// How many spans the sequence holds.
    ///
    /// Counts the spans that remain **after** merging, so inserting several
    /// overlapping spans may leave a length of one.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// uptime.insert(TimeIntervalEvent::span(5, 20)?);
    ///
    /// assert_eq!(uptime.len(), 1);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    // Not a `const fn`: `Vec::len` is const-stable only from 1.87, past this
    // crate's 1.83 MSRV.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether the sequence covers no instants at all.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// assert!(uptime.is_empty());
    ///
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// assert!(!uptime.is_empty());
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    // Not a `const fn`, for the same reason as `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Drop every span, leaving the sequence covering nothing.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// uptime.clear();
    ///
    /// assert!(uptime.is_empty());
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn clear(&mut self) {
        self.spans.clear();
    }

    /// The whole sequence as a slice, earliest span first.
    ///
    /// Constant time — the spans already sit contiguously.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// assert_eq!(uptime.as_slice().len(), 2);
    /// assert_eq!(uptime.as_slice()[0].bounds(), (0, 10));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn as_slice(&self) -> &[TimeIntervalEvent<()>] {
        &self.spans
    }

    /// Mark `span` as covered, merging it into what is already there.
    ///
    /// Spans that overlap **or merely touch** the new one are absorbed into a
    /// single span, since touching spans leave no instant between them.
    /// Inserting a span already covered changes nothing. Logarithmic to locate,
    /// then a shift.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// uptime.insert(TimeIntervalEvent::span(0, 5)?);
    /// uptime.insert(TimeIntervalEvent::span(20, 30)?);
    ///
    /// // Touching the first span: no instant separates them, so they merge.
    /// uptime.insert(TimeIntervalEvent::span(5, 9)?);
    /// assert_eq!(uptime[0].bounds(), (0, 9));
    ///
    /// // A span reaching across the gap swallows both.
    /// uptime.insert(TimeIntervalEvent::span(7, 25)?);
    /// assert_eq!(uptime.len(), 1);
    /// assert_eq!(uptime[0].bounds(), (0, 30));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn insert(&mut self, span: TimeIntervalEvent<()>) {
        // Both comparisons are non-strict, which is what makes merely touching
        // spans merge rather than sit adjacent. The invariant makes everything
        // between these two indices a contiguous run.
        let absorb_from = self
            .spans
            .partition_point(|existing| existing.end() < span.start());
        let absorb_to = self
            .spans
            .partition_point(|existing| existing.start() <= span.end());

        if absorb_from == absorb_to {
            self.spans.insert(absorb_from, span);
            return;
        }

        let start = self.spans[absorb_from].start().min(span.start());
        let end = self.spans[absorb_to - 1].end().max(span.end());

        self.spans
            .splice(absorb_from..absorb_to, [TimeIntervalEvent::raw(start, end)]);
    }

    /// Remove the span at position `index` and return it.
    ///
    /// Removing a span from a disjoint set leaves it disjoint, so the rest of
    /// the sequence is untouched.
    ///
    /// # Panics
    ///
    /// If `index` is past the end, like `Vec::remove`.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(0, 10)?,
    ///     TimeIntervalEvent::span(30, 40)?,
    /// ]);
    ///
    /// assert_eq!(uptime.remove(0).bounds(), (0, 10));
    /// assert_eq!(uptime.len(), 1);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn remove(&mut self, index: usize) -> TimeIntervalEvent<()> {
        self.spans.remove(index)
    }

    /// The span at position `index`, counting from the earliest.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// assert_eq!(uptime.nth(0).map(TimeIntervalEvent::bounds), Some((0, 10)));
    /// assert_eq!(uptime.nth(5), None);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn nth(&self, index: usize) -> Option<&TimeIntervalEvent<()>> {
        self.spans.get(index)
    }

    /// Walk every span, earliest first.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// let bounds: Vec<(i64, i64)> = uptime.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 10), (30, 40)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn iter(&self) -> slice::Iter<'_, TimeIntervalEvent<()>> {
        self.spans.iter()
    }

    /// The earliest span, or `None` when the sequence covers nothing.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// assert_eq!(uptime.first().map(TimeIntervalEvent::start), Some(0));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn first(&self) -> Option<&TimeIntervalEvent<()>> {
        self.spans.first()
    }

    /// The latest span, or `None` when the sequence covers nothing.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// assert_eq!(uptime.last().map(TimeIntervalEvent::end), Some(40));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn last(&self) -> Option<&TimeIntervalEvent<()>> {
        self.spans.last()
    }

    /// The span covering `timestamp`, or `None` when the state was inactive.
    ///
    /// At most one span can cover an instant, because the sequence is disjoint.
    /// Spans are half-open, so a span's `start` is covered and its `end` is
    /// not. Logarithmic.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(10, 20)?]);
    ///
    /// assert_eq!(uptime.at(15).map(TimeIntervalEvent::bounds), Some((10, 20)));
    /// assert_eq!(uptime.at(10).map(TimeIntervalEvent::bounds), Some((10, 20)));
    /// assert_eq!(uptime.at(20), None);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn at(&self, timestamp: Timestamp) -> Option<&TimeIntervalEvent<()>> {
        // The only candidate is the last span starting at or before the
        // instant; anything earlier ended before it, anything later starts
        // after it.
        let candidate = self
            .spans
            .partition_point(|span| span.start() <= timestamp)
            .checked_sub(1)?;

        self.spans
            .get(candidate)
            .filter(|span| span.end() > timestamp)
    }

    /// Whether the state was active at `timestamp`.
    ///
    /// Half-open, as everywhere: a span covers its `start` but not its `end`.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(10, 20)?]);
    ///
    /// assert!(uptime.contains(10));
    /// assert!(uptime.contains(19));
    /// assert!(!uptime.contains(20));
    /// assert!(!uptime.contains(5));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        self.at(timestamp).is_some()
    }
}

impl FromIterator<TimeIntervalEvent<()>> for TimeIntervalSequence {
    /// Collect spans into a sequence, in any order and overlapping freely.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime: TimeIntervalSequence = [
    ///     TimeIntervalEvent::span(5, 20)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(uptime.len(), 1);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    fn from_iter<I: IntoIterator<Item = TimeIntervalEvent<()>>>(spans: I) -> Self {
        Self::from_spans(spans.into_iter().collect())
    }
}

impl Extend<TimeIntervalEvent<()>> for TimeIntervalSequence {
    /// Mark every span as covered, merging into what is already there.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// uptime.extend([TimeIntervalEvent::span(5, 20)?, TimeIntervalEvent::span(40, 50)?]);
    ///
    /// assert_eq!(uptime.len(), 2);
    /// assert_eq!(uptime[0].bounds(), (0, 20));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    fn extend<I: IntoIterator<Item = TimeIntervalEvent<()>>>(&mut self, spans: I) {
        // Appending and renormalizing beats inserting one at a time: it shifts
        // the tail once rather than once per span.
        let mut combined = std::mem::take(&mut self.spans);
        combined.extend(spans);

        *self = Self::from_spans(combined);
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
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 10)?]);
    ///
    /// assert_eq!(uptime[0].bounds(), (0, 10));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
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
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(30, 40)?,
    ///     TimeIntervalEvent::span(0, 10)?,
    /// ]);
    ///
    /// let bounds: Vec<(i64, i64)> = uptime.into_iter().map(|s| s.bounds()).collect();
    /// assert_eq!(bounds, [(0, 10), (30, 40)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    fn into_iter(self) -> Self::IntoIter {
        self.spans.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::TimeIntervalSequence;
    use crate::primitives::TimeIntervalEvent;

    /// A span, for tests where the bounds are known good.
    fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
        TimeIntervalEvent::span(start, end).expect("test bounds are ordered")
    }

    /// Build a sequence from `(start, end)` pairs through `from_spans`.
    fn from_spans(bounds: impl IntoIterator<Item = (i64, i64)>) -> TimeIntervalSequence {
        let sequence = TimeIntervalSequence::from_spans(
            bounds
                .into_iter()
                .map(|(start, end)| span(start, end))
                .collect(),
        );
        assert_normalized(&sequence);

        sequence
    }

    /// Build a sequence by inserting `(start, end)` pairs one at a time,
    /// checking the invariant holds after every step.
    fn inserted(bounds: impl IntoIterator<Item = (i64, i64)>) -> TimeIntervalSequence {
        let mut sequence = TimeIntervalSequence::new();
        for (start, end) in bounds {
            sequence.insert(span(start, end));
            assert_normalized(&sequence);
        }

        sequence
    }

    /// The bounds a sequence reads, in order.
    fn bounds(sequence: &TimeIntervalSequence) -> Vec<(i64, i64)> {
        sequence.iter().map(TimeIntervalEvent::bounds).collect()
    }

    /// The invariant every method must restore: sorted, disjoint, and with no
    /// two spans left touching.
    fn assert_normalized(sequence: &TimeIntervalSequence) {
        for pair in sequence.as_slice().windows(2) {
            assert!(
                pair[0].end() < pair[1].start(),
                "spans {:?} and {:?} should have merged",
                pair[0].bounds(),
                pair[1].bounds(),
            );
        }
    }

    #[test]
    fn disjoint_spans_are_all_kept_in_order() {
        assert_eq!(bounds(&inserted([(30, 40), (0, 10)])), [(0, 10), (30, 40)]);
        assert_eq!(
            bounds(&from_spans([(30, 40), (0, 10)])),
            [(0, 10), (30, 40)]
        );
    }

    #[test]
    fn overlapping_spans_merge() {
        assert_eq!(bounds(&inserted([(0, 10), (5, 20)])), [(0, 20)]);
        assert_eq!(bounds(&from_spans([(0, 10), (5, 20)])), [(0, 20)]);
    }

    #[test]
    fn touching_spans_merge() {
        // No instant separates [0, 5) from [5, 9), so they cover exactly [0, 9).
        assert_eq!(bounds(&inserted([(0, 5), (5, 9)])), [(0, 9)]);
        assert_eq!(bounds(&inserted([(5, 9), (0, 5)])), [(0, 9)]);
        assert_eq!(bounds(&from_spans([(5, 9), (0, 5)])), [(0, 9)]);
    }

    #[test]
    fn a_span_barely_short_of_touching_stays_separate() {
        assert_eq!(bounds(&inserted([(0, 5), (6, 9)])), [(0, 5), (6, 9)]);
    }

    #[test]
    fn overlap_on_either_side_extends_the_existing_span() {
        assert_eq!(bounds(&inserted([(10, 20), (15, 30)])), [(10, 30)]);
        assert_eq!(bounds(&inserted([(10, 20), (0, 15)])), [(0, 20)]);
    }

    #[test]
    fn a_contained_span_changes_nothing() {
        let uptime = inserted([(0, 20), (5, 10)]);

        assert_eq!(bounds(&uptime), [(0, 20)]);
        assert_eq!(uptime.len(), 1);
    }

    #[test]
    fn an_identical_span_changes_nothing() {
        assert_eq!(bounds(&inserted([(0, 20), (0, 20)])), [(0, 20)]);
    }

    #[test]
    fn a_containing_span_replaces_what_it_covers() {
        assert_eq!(bounds(&inserted([(5, 10), (0, 20)])), [(0, 20)]);
    }

    #[test]
    fn a_long_span_swallows_every_span_it_reaches() {
        let uptime = inserted([(0, 5), (10, 15), (20, 25), (30, 35), (2, 32)]);

        assert_eq!(bounds(&uptime), [(0, 35)]);
        assert_eq!(uptime.len(), 1);
    }

    #[test]
    fn a_span_bridging_two_others_by_touching_them_merges_all_three() {
        assert_eq!(bounds(&inserted([(0, 5), (10, 15), (5, 10)])), [(0, 15)]);
    }

    #[test]
    fn a_span_landing_in_a_gap_stays_separate() {
        let uptime = inserted([(0, 5), (30, 40), (10, 20)]);

        assert_eq!(bounds(&uptime), [(0, 5), (10, 20), (30, 40)]);
    }

    #[test]
    fn insertion_and_bulk_construction_agree() {
        let spans = [(30, 40), (0, 10), (5, 20), (35, 50), (-10, -5)];

        assert_eq!(inserted(spans), from_spans(spans));
    }

    #[test]
    fn sequences_covering_the_same_instants_are_equal() {
        let piecemeal = from_spans([(0, 5), (5, 10), (2, 7)]);
        let whole = from_spans([(0, 10)]);

        assert_eq!(piecemeal, whole);
        assert_eq!(piecemeal.len(), 1);
    }

    #[test]
    fn len_counts_spans_after_merging_not_inserts() {
        let uptime = inserted([(0, 10), (5, 20), (15, 30)]);

        assert_eq!(uptime.len(), 1);
    }

    #[test]
    fn negative_bounds_are_ordered_and_merged() {
        assert_eq!(
            bounds(&inserted([(-5, 5), (-20, -10), (-12, -3)])),
            [(-20, 5)]
        );
    }

    #[test]
    fn the_timestamp_extremes_merge_without_overflowing() {
        // Merging only compares and takes min/max, so the widest possible spans
        // are no different from any others.
        let uptime = inserted([(i64::MIN, 0), (-1, i64::MAX)]);

        assert_eq!(bounds(&uptime), [(i64::MIN, i64::MAX)]);
    }

    #[test]
    fn remove_takes_the_span_at_that_position() {
        let mut uptime = from_spans([(0, 10), (30, 40), (50, 60)]);

        assert_eq!(uptime.remove(1).bounds(), (30, 40));
        assert_eq!(bounds(&uptime), [(0, 10), (50, 60)]);
        assert_normalized(&uptime);
    }

    #[test]
    #[should_panic(expected = "removal index")]
    fn removing_past_the_end_panics() {
        let mut uptime = from_spans([(0, 10)]);

        let _ = uptime.remove(5);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn indexing_past_the_end_panics() {
        let uptime = from_spans([(0, 10)]);

        let _ = uptime[5];
    }

    #[test]
    fn a_span_covers_its_start_but_not_its_end() {
        let uptime = from_spans([(10, 20)]);

        assert!(uptime.contains(10));
        assert!(uptime.contains(19));
        assert!(!uptime.contains(20));
        assert_eq!(uptime.at(10).map(TimeIntervalEvent::bounds), Some((10, 20)));
        assert!(uptime.at(20).is_none());
    }

    #[test]
    fn lookups_find_nothing_in_a_gap_or_past_the_edges() {
        let uptime = from_spans([(10, 20), (30, 40)]);

        assert!(!uptime.contains(25));
        assert!(!uptime.contains(5));
        assert!(!uptime.contains(50));
        assert!(uptime.at(25).is_none());
    }

    #[test]
    fn lookups_pick_the_right_span_among_several() {
        let uptime = from_spans([(0, 10), (30, 40), (50, 60)]);

        assert_eq!(uptime.at(35).map(TimeIntervalEvent::bounds), Some((30, 40)));
        assert_eq!(uptime.at(55).map(TimeIntervalEvent::bounds), Some((50, 60)));
        assert_eq!(uptime.at(5).map(TimeIntervalEvent::bounds), Some((0, 10)));
    }

    #[test]
    fn first_and_last_bracket_the_sequence() {
        let uptime = from_spans([(30, 40), (0, 10)]);

        assert_eq!(uptime.first().map(TimeIntervalEvent::bounds), Some((0, 10)));
        assert_eq!(uptime.last().map(TimeIntervalEvent::bounds), Some((30, 40)));
    }

    #[test]
    fn nth_reads_by_position() {
        let uptime = from_spans([(30, 40), (0, 10)]);

        assert_eq!(uptime.nth(0).map(TimeIntervalEvent::bounds), Some((0, 10)));
        assert_eq!(uptime.nth(1).map(TimeIntervalEvent::bounds), Some((30, 40)));
        assert!(uptime.nth(2).is_none());
        assert_eq!(uptime[1].bounds(), (30, 40));
    }

    #[test]
    fn an_empty_sequence_answers_nothing() {
        let uptime = TimeIntervalSequence::new();

        assert!(uptime.is_empty());
        assert_eq!(uptime.len(), 0);
        assert!(uptime.as_slice().is_empty());
        assert!(uptime.nth(0).is_none());
        assert!(uptime.first().is_none());
        assert!(uptime.last().is_none());
        assert!(uptime.at(0).is_none());
        assert!(!uptime.contains(0));
        assert_eq!(uptime.iter().count(), 0);
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(TimeIntervalSequence::new(), TimeIntervalSequence::default());
    }

    #[test]
    fn from_spans_accepts_nothing() {
        let uptime = TimeIntervalSequence::from_spans(vec![]);

        assert!(uptime.is_empty());
        assert_eq!(uptime, TimeIntervalSequence::new());
    }

    #[test]
    fn clear_empties_the_sequence() {
        let mut uptime = from_spans([(0, 10), (30, 40)]);
        uptime.clear();

        assert!(uptime.is_empty());
    }

    #[test]
    fn collecting_round_trips_through_into_iter() {
        let uptime = from_spans([(30, 40), (0, 10), (5, 20)]);

        let spans: Vec<TimeIntervalEvent<()>> = uptime.clone().into_iter().collect();
        assert_eq!(spans.len(), 2);

        let rebuilt: TimeIntervalSequence = spans.into_iter().collect();
        assert_eq!(rebuilt, uptime);
    }

    #[test]
    fn into_spans_round_trips_from_spans() {
        let uptime = from_spans([(0, 10), (5, 20), (30, 40)]);
        let spans = uptime.clone().into_spans();

        assert_eq!(spans.len(), 2);
        assert_eq!(TimeIntervalSequence::from_spans(spans), uptime);
    }

    #[test]
    fn extend_normalizes_against_what_is_already_there() {
        let mut uptime = inserted([(0, 10)]);
        uptime.extend([span(5, 20), span(40, 50)]);
        assert_normalized(&uptime);

        assert_eq!(bounds(&uptime), [(0, 20), (40, 50)]);
    }

    #[test]
    fn extend_onto_an_empty_sequence_still_normalizes() {
        let mut uptime = TimeIntervalSequence::new();
        uptime.extend([span(5, 20), span(0, 10)]);
        assert_normalized(&uptime);

        assert_eq!(bounds(&uptime), [(0, 20)]);
    }

    #[test]
    fn borrowed_iteration_leaves_the_sequence_usable() {
        let uptime = from_spans([(0, 10), (30, 40)]);

        let seen: Vec<(i64, i64)> = (&uptime)
            .into_iter()
            .map(TimeIntervalEvent::bounds)
            .collect();

        assert_eq!(seen, [(0, 10), (30, 40)]);
        assert_eq!(uptime.len(), 2);
    }
}
