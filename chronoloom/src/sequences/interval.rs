//! A normalized timeline of the spans during which one state was active.

use std::ops::Index;
use std::{slice, vec};

use crate::primitives::{IntervalError, TimeIntervalEvent, Timestamp};

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
/// # Active duration
///
/// The sequence carries the total time it covers as it goes, so
/// [`active_duration`] is a read rather than a walk. Normalization is what
/// makes that total meaningful: because the spans are disjoint, it is their
/// plain sum, and an instant covered by three overlapping inserts still counts
/// once.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let uptime = TimeIntervalSequence::from_spans(vec![
///     TimeIntervalEvent::span(0, 10)?,
///     TimeIntervalEvent::span(5, 20)?,
///     TimeIntervalEvent::span(30, 40)?,
/// ]);
///
/// // `[0, 20)` and `[30, 40)`: 20 ticks and 10, not 10 + 15 + 10.
/// assert_eq!(uptime.active_duration(), 30);
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// [`active_duration`]: TimeIntervalSequence::active_duration
/// [`len`]: TimeIntervalSequence::len
#[derive(Debug, Clone, Default)]
pub struct TimeIntervalSequence {
    /// Normalized: sorted by start, pairwise disjoint, and never touching — so
    /// `spans[i].end() < spans[i + 1].start()` strictly, for every adjacent
    /// pair. Every method restores this before returning.
    spans: Vec<TimeIntervalEvent<()>>,
    /// Total ticks covered by `spans`. Disjointness makes this their plain sum,
    /// with no instant counted twice; every method that touches `spans` updates
    /// it in the same breath.
    active_duration: u64,
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
        Self {
            spans: Vec::new(),
            active_duration: 0,
        }
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

        // Sweep once, folding each span into the one still open. The sort
        // guarantees every span that can merge with the current one comes next,
        // so a single pass is enough.
        let mut normalized: Vec<TimeIntervalEvent<()>> = Vec::with_capacity(spans.len());
        for span in spans {
            absorb(&mut normalized, span);
        }

        Self::from_normalized(normalized)
    }

    /// Wrap spans that are already in canonical form.
    ///
    /// Private and unchecked: the caller must have produced them sorted,
    /// disjoint, and non-touching. Every use here either folds through
    /// [`absorb`] or walks two already-normalized sequences in order, both of
    /// which produce canonical output — so re-sorting would be wasted work.
    ///
    /// The one pass this does make is totalling the duration. Every caller is
    /// already linear in the spans, so it costs nothing asymptotically, and
    /// funnelling them all through here means no construction path can forget.
    fn from_normalized(spans: Vec<TimeIntervalEvent<()>>) -> Self {
        let active_duration = spans.iter().fold(0u64, |total, span| {
            total.checked_add(span_ticks(span)).expect(OVERFLOW)
        });

        Self {
            spans,
            active_duration,
        }
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

    /// How much time the sequence covers in total, in the same ticks as its
    /// bounds.
    ///
    /// Maintained as spans arrive and leave rather than computed on demand, so
    /// this is a constant-time read. It counts *covered instants*, not inserted
    /// spans: overlapping inserts contribute their union once, and touching
    /// spans contribute the whole they merge into. An empty sequence covers
    /// nothing and reports `0`.
    ///
    /// Unsigned, because a total is a magnitude, and never saturating: spans
    /// live inside `i64` and never overlap, so the total cannot exceed
    /// `i64::MAX - i64::MIN`, which is exactly [`u64::MAX`]. That makes `u64`
    /// wide enough for every sequence that can be built, where
    /// [`TimeIntervalEvent::duration`] must saturate to stay in `i64`.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new();
    /// assert_eq!(uptime.active_duration(), 0);
    ///
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// assert_eq!(uptime.active_duration(), 10);
    ///
    /// // Overlapping: only the five new ticks count.
    /// uptime.insert(TimeIntervalEvent::span(5, 15)?);
    /// assert_eq!(uptime.active_duration(), 15);
    ///
    /// // Already covered: nothing to add.
    /// uptime.insert(TimeIntervalEvent::span(2, 8)?);
    /// assert_eq!(uptime.active_duration(), 15);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub const fn active_duration(&self) -> u64 {
        self.active_duration
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
    /// assert_eq!(uptime.active_duration(), 0);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn clear(&mut self) {
        self.spans.clear();
        self.active_duration = 0;
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
    /// [`active_duration`] grows by the instants this span newly covers, so
    /// re-inserting covered ground leaves it alone.
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
    ///
    /// [`active_duration`]: TimeIntervalSequence::active_duration
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
            // The span landed in a gap, so all of it is newly covered.
            self.active_duration = self
                .active_duration
                .checked_add(span_ticks(&span))
                .expect(OVERFLOW);
            self.spans.insert(absorb_from, span);
            return;
        }

        let start = self.spans[absorb_from].start().min(span.start());
        let end = self.spans[absorb_to - 1].end().max(span.end());
        let merged = TimeIntervalEvent::raw(start, end);

        // The merged span covers everything the absorbed run did and possibly
        // more, so trade the run's total for the whole. Swapping the two rather
        // than working out what is new keeps this free of the overlap
        // arithmetic `partition_point` already settled.
        let absorbed = self.spans[absorb_from..absorb_to]
            .iter()
            .fold(0u64, |total, existing| {
                total.checked_add(span_ticks(existing)).expect(OVERFLOW)
            });
        self.active_duration = self
            .active_duration
            .checked_sub(absorbed)
            .expect(UNDERFLOW)
            .checked_add(span_ticks(&merged))
            .expect(OVERFLOW);

        self.spans.splice(absorb_from..absorb_to, [merged]);
    }

    /// Remove the span at position `index` and return it.
    ///
    /// Removing a span from a disjoint set leaves it disjoint, so the rest of
    /// the sequence is untouched. [`active_duration`] drops by exactly what the
    /// removed span covered, since no other span shared any of it.
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
    /// assert_eq!(uptime.active_duration(), 10);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`active_duration`]: TimeIntervalSequence::active_duration
    pub fn remove(&mut self, index: usize) -> TimeIntervalEvent<()> {
        let removed = self.spans.remove(index);
        self.active_duration = self
            .active_duration
            .checked_sub(span_ticks(&removed))
            .expect(UNDERFLOW);

        removed
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

    /// The instants covered by **either** timeline.
    ///
    /// Both operands are borrowed and left untouched; the result is a new
    /// sequence. Spans that touch across the two merge, as they would within
    /// one: `[0, 5)` here and `[5, 9)` there become `[0, 9)`.
    ///
    /// Both sequences are already ordered, so this is a single pass over the
    /// two — linear in their combined length, with no sorting.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 5)?]);
    /// let extra = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(5, 9)?,
    ///     TimeIntervalEvent::span(20, 30)?,
    /// ]);
    ///
    /// let either = up.union(&extra);
    /// let bounds: Vec<(i64, i64)> = either.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 9), (20, 30)]);
    ///
    /// // Neither operand changed.
    /// assert_eq!(up.len(), 1);
    /// assert_eq!(extra.len(), 2);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        // At most every span from both sides survives, so this capacity is an
        // exact upper bound.
        let mut spans = Vec::with_capacity(self.spans.len() + other.spans.len());
        let (mut i, mut j) = (0, 0);

        loop {
            // Take whichever side starts earlier. A tie can go either way,
            // since the two will merge regardless.
            let next = match (self.spans.get(i), other.spans.get(j)) {
                (Some(mine), Some(theirs)) => {
                    if mine.start() <= theirs.start() {
                        i += 1;
                        *mine
                    } else {
                        j += 1;
                        *theirs
                    }
                }
                (Some(mine), None) => {
                    i += 1;
                    *mine
                }
                (None, Some(theirs)) => {
                    j += 1;
                    *theirs
                }
                (None, None) => break,
            };

            absorb(&mut spans, next);
        }

        Self::from_normalized(spans)
    }

    /// The instants covered by **both** timelines.
    ///
    /// Both operands are borrowed and left untouched; the result is a new
    /// sequence. Because spans are half-open, timelines that merely touch share
    /// no instant and so intersect to nothing.
    ///
    /// A single pass over the two, linear in their combined length.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 100)?]);
    /// let busy = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(10, 20)?,
    ///     TimeIntervalEvent::span(30, 40)?,
    /// ]);
    ///
    /// // Everything busy happened while up, so the overlap is `busy` exactly.
    /// assert_eq!(up.intersection(&busy), busy);
    ///
    /// let touching = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(100, 200)?]);
    /// assert!(up.intersection(&touching).is_empty());
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        let mut spans = Vec::new();
        let (mut i, mut j) = (0, 0);

        while let (Some(mine), Some(theirs)) = (self.spans.get(i), other.spans.get(j)) {
            // What the pair shares is the primitive's business, not this loop's.
            if let Some(overlap) = mine.intersection(theirs) {
                spans.push(overlap);
            }

            // Whichever ends first cannot reach anything still to come, so it is
            // safe to drop. That is the entire trick.
            if mine.end() < theirs.end() {
                i += 1;
            } else {
                j += 1;
            }
        }

        Self::from_normalized(spans)
    }

    /// The instants covered by this timeline but **not** by `other`.
    ///
    /// Both operands are borrowed and left untouched; the result is a new
    /// sequence. A span of `other` landing inside one of ours splits it in two.
    ///
    /// A single pass over the two, linear in their combined length.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 100)?]);
    /// let maintenance = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(10, 20)?,
    ///     TimeIntervalEvent::span(30, 40)?,
    /// ]);
    ///
    /// let serving = up.difference(&maintenance);
    /// let bounds: Vec<(i64, i64)> = serving.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 10), (20, 30), (40, 100)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn difference(&self, other: &Self) -> Self {
        // `cursor` is the first instant of the current span not yet accounted
        // for. Starting it needs a first span, so an empty timeline is done
        // before the loop begins.
        let Some(first) = self.spans.first() else {
            return Self::new();
        };

        let mut spans = Vec::new();
        let (mut i, mut j) = (0, 0);
        let mut cursor = first.start();

        while let Some(mine) = self.spans.get(i) {
            match other.spans.get(j) {
                // Nothing of `other` reaches into what is left of this span.
                Some(theirs) if theirs.start() < mine.end() => {
                    if theirs.end() <= cursor {
                        // Already behind the cursor, so it covers nothing new.
                        j += 1;
                        continue;
                    }

                    if cursor < theirs.start() {
                        spans.push(TimeIntervalEvent::raw(cursor, theirs.start()));
                    }
                    // The guard above established `theirs.end() > cursor`, so
                    // this only ever moves the cursor forward.
                    cursor = theirs.end();

                    if cursor >= mine.end() {
                        // This span is fully accounted for, but `theirs` may
                        // still reach into the next one.
                        i += 1;
                    } else {
                        j += 1;
                    }
                }
                _ => {
                    if cursor < mine.end() {
                        spans.push(TimeIntervalEvent::raw(cursor, mine.end()));
                    }
                    i += 1;
                }
            }

            // Every new span starts uncovered.
            if let Some(next) = self.spans.get(i) {
                cursor = cursor.max(next.start());
            }
        }

        Self::from_normalized(spans)
    }

    /// The instants covered by exactly **one** of the two timelines.
    ///
    /// The set-algebra XOR: an instant belongs to the result when it is in this
    /// timeline or the other, but not both. Both operands are borrowed and left
    /// untouched.
    ///
    /// Composed as `(self - other) ∪ (other - self)`, so still linear in the
    /// combined length.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let a = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(0, 10)?]);
    /// let b = TimeIntervalSequence::from_spans(vec![TimeIntervalEvent::span(5, 15)?]);
    ///
    /// let only_one = a.symmetric_difference(&b);
    /// let bounds: Vec<(i64, i64)> = only_one.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 5), (10, 15)]);
    ///
    /// // Exactly the instants where the two disagree.
    /// assert!(only_one.contains(2));
    /// assert!(!only_one.contains(7));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        self.difference(other).union(&other.difference(self))
    }

    /// The timeline with every span's bounds moved: `alpha` shifts the lower
    /// bound, `beta` the upper.
    ///
    /// Written `A[alpha, beta]` for a timeline `A`, this turns every span
    /// `[s, e)` into `[s + alpha, e + beta)`. Both shifts may be negative, and
    /// both move the bound to the right when positive. `A[0, 0]` is `A`, and
    /// `A[t, t]` is `A` translated by `t`.
    ///
    /// Because the two bounds move independently, a span can be widened until
    /// it swallows the gap after it, or narrowed until nothing is left of it —
    /// so the result is re-normalized, not merely re-bounded.
    ///
    /// # Narrows or widens, never both
    ///
    /// Only the difference `delta = alpha - beta` decides what happens
    /// structurally. A span survives exactly when it is wider than `delta`, and
    /// a gap of `g` closes exactly when `g + delta <= 0`. So a call that can
    /// drop spans (`delta > 0`) also widens every gap and can merge nothing,
    /// and a call that can merge (`delta < 0`) also widens every span and can
    /// drop nothing.
    ///
    /// Widening merges the spans it brings together — and a gap closed to
    /// exactly zero counts, since touching spans cover an unbroken stretch:
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let alerts = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(0, 10)?,
    ///     TimeIntervalEvent::span(14, 20)?,
    /// ]);
    ///
    /// // Two ticks of slack on each side closes the four-tick gap exactly.
    /// let within_two = alerts.transform(-2, 2)?;
    /// let bounds: Vec<(i64, i64)> = within_two.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(-2, 22)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// Narrowing drops the spans it consumes entirely:
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(0, 3)?,
    ///     TimeIntervalEvent::span(10, 30)?,
    /// ]);
    ///
    /// // Only stretches longer than three ticks have a solid interior left.
    /// let sustained = up.transform(3, 0)?;
    /// let bounds: Vec<(i64, i64)> = sustained.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(13, 30)]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// A single pass over the spans, linear in their number, with no sorting:
    /// shifting every bound by the same amount leaves them in the order they
    /// were already in.
    ///
    /// # Coverage, not history
    ///
    /// The spans moved are the normalized ones, so `[0, 5)` and `[5, 10)` are
    /// one `[0, 10)` span by the time this sees them and a narrowing takes one
    /// span's worth of time from them, not two. For the same reason transforms
    /// compose — `A[a1, b1][a2, b2]` is `A[a1 + a2, b1 + b2]` — only while
    /// nothing merges or vanishes: both discard structure that no later
    /// transform can recover.
    ///
    /// # Errors
    ///
    /// Returns [`IntervalError::BoundOverflow`] if any shifted bound leaves the
    /// range a [`Timestamp`] can hold. Nothing is transformed in that case; the
    /// receiver is borrowed and never changes either way.
    pub fn transform(&self, alpha: Timestamp, beta: Timestamp) -> Result<Self, IntervalError> {
        // Merging only ever removes spans, so this capacity is an upper bound.
        let mut spans = Vec::with_capacity(self.spans.len());

        for span in &self.spans {
            let (start, end) = span.bounds();
            let start = shifted(start, alpha)?;
            let end = shifted(end, beta)?;

            // The bounds crossed, so the span narrowed away to nothing.
            if start < end {
                // Both bounds moved by a fixed amount, so the shifted spans
                // arrive in the order they were stored in and the tail is the
                // only one a new span can reach.
                absorb(&mut spans, TimeIntervalEvent::raw(start, end));
            }
        }

        Ok(Self::from_normalized(spans))
    }
}

/// Both of these are unreachable rather than unlikely — see [`span_ticks`] for
/// why the sum always fits, and the field doc on `active_duration` for why it
/// always accounts for exactly the spans present. They exist so a bug in that
/// bookkeeping surfaces here instead of as a silently wrong total.
const OVERFLOW: &str = "covered ticks always fit in u64: spans are disjoint and bounded by i64";
const UNDERFLOW: &str = "active_duration always accounts for every span present";

/// Move `bound` by `shift`, or say that it cannot be moved.
///
/// The only arithmetic here a caller can push past [`Timestamp`]'s range: every
/// other total is bounded by spans that already exist, whereas a shift is a
/// number handed in from outside. So this reports rather than panics, and
/// [`TimeIntervalSequence::transform`] hands the failure to its caller.
fn shifted(bound: Timestamp, shift: Timestamp) -> Result<Timestamp, IntervalError> {
    bound
        .checked_add(shift)
        .ok_or(IntervalError::BoundOverflow { bound, shift })
}

/// The ticks a span covers, exactly.
///
/// [`TimeIntervalEvent::duration`] saturates at [`i64::MAX`] to stay in `i64`,
/// which a running total must not do. A half-open span inside `i64` is at most
/// `i64::MAX - i64::MIN` ticks wide — exactly [`u64::MAX`] — so `u64` holds
/// every one of them, and the widest span reports its true width rather than a
/// clamped one.
fn span_ticks(span: &TimeIntervalEvent<()>) -> u64 {
    let (start, end) = span.bounds();

    // Via `i128` because the difference can exceed `i64`; the subtraction
    // cannot be negative, since an interval always ends after it starts.
    u64::try_from(i128::from(end) - i128::from(start)).expect("a span always ends after it starts")
}

/// Fold `span` into an already-normalized `normalized`, merging when the two
/// combine.
///
/// `span` must start at or after the last span in `normalized`, which every
/// caller guarantees by walking its input in order. Whether two spans combine,
/// and into what, is [`TimeIntervalEvent::merged`]'s decision — this exists so
/// the sequence never restates that rule.
fn absorb(normalized: &mut Vec<TimeIntervalEvent<()>>, span: TimeIntervalEvent<()>) {
    match normalized.pop() {
        Some(open) => match open.merged(&span) {
            Some(merged) => normalized.push(merged),
            None => {
                normalized.push(open);
                normalized.push(span);
            }
        },
        None => normalized.push(span),
    }
}

impl PartialEq for TimeIntervalSequence {
    /// Two sequences are equal exactly when they cover the same instants.
    ///
    /// Written out rather than derived so the spans alone decide. The duration
    /// is a function of them — comparing it could only ever agree, or reveal a
    /// bookkeeping bug by disagreeing, and equality is the wrong place to
    /// discover that.
    fn eq(&self, other: &Self) -> bool {
        self.spans == other.spans
    }
}

impl Eq for TimeIntervalSequence {}

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
    use super::{span_ticks, TimeIntervalSequence};
    use crate::primitives::{IntervalError, TimeIntervalEvent};

    /// A pair of operands, each as its `(start, end)` bounds.
    type OperandShapes<'a> = (&'a [(i64, i64)], &'a [(i64, i64)]);

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

    /// Assert an operation means what it claims, one instant at a time.
    ///
    /// Rather than trusting a hand-written list of expected spans — which can
    /// encode the very off-by-one it is meant to catch — this checks that the
    /// result covers exactly the instants the boolean rule says it should,
    /// across the whole range the operands touch and a margin either side.
    fn assert_covers(
        result: &TimeIntervalSequence,
        a: &TimeIntervalSequence,
        b: &TimeIntervalSequence,
        rule: impl Fn(bool, bool) -> bool,
    ) {
        assert_normalized(result);

        for timestamp in -5..=105 {
            assert_eq!(
                result.contains(timestamp),
                rule(a.contains(timestamp), b.contains(timestamp)),
                "instant {timestamp} is on the wrong side of the result",
            );
        }
    }

    /// Transform a timeline by shifts known to stay in range.
    fn transformed(sequence: &TimeIntervalSequence, alpha: i64, beta: i64) -> TimeIntervalSequence {
        let result = sequence
            .transform(alpha, beta)
            .expect("test shifts stay inside the timestamp range");
        assert_normalized(&result);

        result
    }

    /// Every operation, checked against its boolean rule on the same operands.
    fn assert_all_operations(a: &TimeIntervalSequence, b: &TimeIntervalSequence) {
        assert_covers(&a.union(b), a, b, |x, y| x || y);
        assert_covers(&a.intersection(b), a, b, |x, y| x && y);
        assert_covers(&a.difference(b), a, b, |x, y| x && !y);
        assert_covers(&a.symmetric_difference(b), a, b, |x, y| x ^ y);
    }

    /// The invariants every method must restore: sorted, disjoint, with no two
    /// spans left touching, and a duration that still totals what is there.
    ///
    /// The duration check lives here, rather than in tests of its own, because
    /// every builder and every operation result already passes through this —
    /// so a mutation path that forgets to update the cache fails the whole
    /// suite rather than only the tests that thought to look.
    fn assert_normalized(sequence: &TimeIntervalSequence) {
        for pair in sequence.as_slice().windows(2) {
            assert!(
                pair[0].end() < pair[1].start(),
                "spans {:?} and {:?} should have merged",
                pair[0].bounds(),
                pair[1].bounds(),
            );
        }

        let recomputed: u64 = sequence.iter().map(span_ticks).sum();
        assert_eq!(
            sequence.active_duration(),
            recomputed,
            "active_duration disagrees with the spans {:?}",
            bounds(sequence),
        );
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
    fn every_operation_means_what_it_says_on_varied_shapes() {
        // Overlapping, nested, touching, disjoint, and interleaved operands,
        // each checked instant by instant against its boolean rule.
        let shapes: [OperandShapes<'_>; 8] = [
            (&[(0, 10)], &[(5, 15)]),
            (&[(0, 100)], &[(10, 20), (30, 40)]),
            (&[(0, 5)], &[(5, 9)]),
            (&[(0, 5)], &[(20, 30)]),
            (&[(0, 10), (20, 30)], &[(5, 25)]),
            (&[(0, 20)], &[(5, 10)]),
            (&[(0, 10), (20, 30), (40, 50)], &[(5, 15), (25, 45)]),
            (&[(10, 20)], &[(10, 20)]),
        ];

        for (left, right) in shapes {
            let a = from_spans(left.iter().copied());
            let b = from_spans(right.iter().copied());

            assert_all_operations(&a, &b);
            assert_all_operations(&b, &a);
        }
    }

    #[test]
    fn every_operation_handles_an_empty_operand_on_either_side() {
        let a = from_spans([(0, 10), (20, 30)]);
        let empty = TimeIntervalSequence::new();

        assert_all_operations(&a, &empty);
        assert_all_operations(&empty, &a);
        assert_all_operations(&empty, &empty);
    }

    #[test]
    fn touching_sequences_unite_but_never_intersect() {
        let a = from_spans([(0, 5)]);
        let b = from_spans([(5, 9)]);

        assert_eq!(bounds(&a.union(&b)), [(0, 9)]);
        assert!(a.intersection(&b).is_empty());
        assert_eq!(bounds(&a.difference(&b)), [(0, 5)]);
        assert_eq!(bounds(&a.symmetric_difference(&b)), [(0, 9)]);
    }

    #[test]
    fn one_long_span_against_many_short_ones() {
        let up = from_spans([(0, 100)]);
        let busy = from_spans([(10, 20), (30, 40)]);

        assert_eq!(up.intersection(&busy), busy);
        assert_eq!(
            bounds(&up.difference(&busy)),
            [(0, 10), (20, 30), (40, 100)]
        );
        assert_eq!(up.union(&busy), up);
        assert_eq!(
            bounds(&up.symmetric_difference(&busy)),
            [(0, 10), (20, 30), (40, 100)]
        );
    }

    #[test]
    fn a_span_straddling_two_of_ours_carves_both() {
        // `theirs` reaches past the end of the first span into the second, so
        // `difference` must advance its own index without dropping `theirs`.
        let mine = from_spans([(0, 10), (20, 30)]);
        let theirs = from_spans([(5, 25)]);

        assert_eq!(bounds(&mine.difference(&theirs)), [(0, 5), (25, 30)]);
        assert_all_operations(&mine, &theirs);
    }

    #[test]
    fn operations_leave_both_operands_untouched() {
        let a = from_spans([(0, 10), (20, 30)]);
        let b = from_spans([(5, 25)]);
        let (a_before, b_before) = (a.clone(), b.clone());

        let _ = a.union(&b);
        let _ = a.intersection(&b);
        let _ = a.difference(&b);
        let _ = a.symmetric_difference(&b);

        assert_eq!(a, a_before);
        assert_eq!(b, b_before);

        // And both are still usable for further work.
        assert_eq!(a.union(&b), a_before.union(&b_before));
    }

    #[test]
    fn operations_are_commutative_where_they_should_be() {
        let a = from_spans([(0, 10), (20, 30)]);
        let b = from_spans([(5, 25), (40, 50)]);

        assert_eq!(a.union(&b), b.union(&a));
        assert_eq!(a.intersection(&b), b.intersection(&a));
        assert_eq!(a.symmetric_difference(&b), b.symmetric_difference(&a));

        // Difference is the one that is not.
        assert_ne!(a.difference(&b), b.difference(&a));
    }

    #[test]
    fn operations_against_itself_collapse() {
        let a = from_spans([(0, 10), (20, 30)]);

        assert_eq!(a.union(&a), a);
        assert_eq!(a.intersection(&a), a);
        assert!(a.difference(&a).is_empty());
        assert!(a.symmetric_difference(&a).is_empty());
    }

    #[test]
    fn the_empty_sequence_is_the_identity_it_should_be() {
        let a = from_spans([(0, 10), (20, 30)]);
        let empty = TimeIntervalSequence::new();

        assert_eq!(a.union(&empty), a);
        assert!(a.intersection(&empty).is_empty());
        assert_eq!(a.difference(&empty), a);
        assert!(empty.difference(&a).is_empty());
        assert_eq!(a.symmetric_difference(&empty), a);
    }

    #[test]
    fn absorption_laws_hold() {
        let a = from_spans([(0, 10), (20, 30)]);
        let b = from_spans([(5, 25)]);

        assert_eq!(a.union(&a.intersection(&b)), a);
        assert_eq!(a.intersection(&a.union(&b)), a);
    }

    #[test]
    fn intersection_distributes_over_union_and_the_dual() {
        let a = from_spans([(0, 20), (40, 60)]);
        let b = from_spans([(10, 30)]);
        let c = from_spans([(15, 50)]);

        assert_eq!(
            a.intersection(&b.union(&c)),
            a.intersection(&b).union(&a.intersection(&c)),
        );
        assert_eq!(
            a.union(&b.intersection(&c)),
            a.union(&b).intersection(&a.union(&c)),
        );
    }

    #[test]
    fn de_morgan_holds_relative_to_the_left_operand() {
        let a = from_spans([(0, 60)]);
        let b = from_spans([(10, 30)]);
        let c = from_spans([(20, 50)]);

        assert_eq!(
            a.difference(&b.union(&c)),
            a.difference(&b).intersection(&a.difference(&c))
        );
        assert_eq!(
            a.difference(&b.intersection(&c)),
            a.difference(&b).union(&a.difference(&c))
        );
    }

    #[test]
    fn the_operations_agree_with_each_other() {
        let a = from_spans([(0, 10), (20, 30), (40, 50)]);
        let b = from_spans([(5, 25), (45, 60)]);

        // Symmetric difference is everything covered, minus what both share.
        assert_eq!(
            a.symmetric_difference(&b),
            a.union(&b).difference(&a.intersection(&b)),
        );
        // What is only ours, plus what we share, is everything of ours.
        assert_eq!(a.difference(&b).union(&a.intersection(&b)), a);
    }

    #[test]
    fn operations_handle_negative_bounds() {
        let a = from_spans([(-20, -10), (-5, 5)]);
        let b = from_spans([(-15, 0)]);

        assert_eq!(bounds(&a.union(&b)), [(-20, 5)]);
        assert_eq!(bounds(&a.intersection(&b)), [(-15, -10), (-5, 0)]);
        assert_eq!(bounds(&a.difference(&b)), [(-20, -15), (0, 5)]);
    }

    #[test]
    fn operations_survive_the_timestamp_extremes() {
        let widest = from_spans([(i64::MIN, i64::MAX)]);
        let middle = from_spans([(-1, 1)]);

        assert_eq!(widest.union(&middle), widest);
        assert_eq!(widest.intersection(&middle), middle);
        assert_eq!(
            bounds(&widest.difference(&middle)),
            [(i64::MIN, -1), (1, i64::MAX)]
        );
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

    #[test]
    fn an_empty_sequence_covers_no_time() {
        assert_eq!(TimeIntervalSequence::new().active_duration(), 0);
        assert_eq!(TimeIntervalSequence::default().active_duration(), 0);
        assert_eq!(from_spans([]).active_duration(), 0);
    }

    #[test]
    fn duration_totals_the_gaps_between_spans_away() {
        assert_eq!(from_spans([(0, 10)]).active_duration(), 10);
        assert_eq!(from_spans([(0, 10), (30, 40)]).active_duration(), 20);
        assert_eq!(from_spans([(-10, -4)]).active_duration(), 6);
    }

    #[test]
    fn duration_counts_covered_instants_not_inserted_spans() {
        // Three spans covering `[0, 20)` between them, however they arrived.
        let overlapping = from_spans([(0, 10), (5, 15), (12, 20)]);
        assert_eq!(overlapping.len(), 1);
        assert_eq!(overlapping.active_duration(), 20);

        // Touching spans merge, and the whole is the sum of its parts here.
        assert_eq!(from_spans([(0, 5), (5, 9)]).active_duration(), 9);

        // The same coverage built any other way agrees.
        assert_eq!(inserted([(12, 20), (0, 10), (5, 15)]).active_duration(), 20);
        assert_eq!(from_spans([(0, 20)]).active_duration(), 20);
    }

    #[test]
    fn inserting_adds_only_what_was_not_covered() {
        let mut uptime = inserted([(0, 10)]);
        assert_eq!(uptime.active_duration(), 10);

        // Into a gap: all of it is new.
        uptime.insert(span(30, 40));
        assert_eq!(uptime.active_duration(), 20);

        // Overlapping: only the part past the existing end.
        uptime.insert(span(5, 15));
        assert_eq!(uptime.active_duration(), 25);

        // Wholly covered: nothing new at all.
        uptime.insert(span(2, 8));
        assert_eq!(uptime.active_duration(), 25);

        // Bridging the gap adds the gap itself, and nothing more.
        uptime.insert(span(15, 30));
        assert_eq!(uptime.len(), 1);
        assert_eq!(uptime.active_duration(), 40);
        assert_normalized(&uptime);
    }

    #[test]
    fn a_span_swallowing_several_trades_them_for_the_whole() {
        let mut uptime = inserted([(0, 10), (20, 30), (40, 50)]);
        assert_eq!(uptime.active_duration(), 30);

        uptime.insert(span(5, 45));

        assert_eq!(bounds(&uptime), [(0, 50)]);
        assert_eq!(uptime.active_duration(), 50);
    }

    #[test]
    fn removing_gives_back_exactly_what_that_span_covered() {
        let mut uptime = from_spans([(0, 10), (30, 40), (50, 55)]);
        assert_eq!(uptime.active_duration(), 25);

        uptime.remove(1);
        assert_eq!(uptime.active_duration(), 15);
        assert_normalized(&uptime);

        uptime.remove(0);
        assert_eq!(uptime.active_duration(), 5);

        uptime.remove(0);
        assert_eq!(uptime.active_duration(), 0);
        assert!(uptime.is_empty());
    }

    #[test]
    fn clearing_resets_the_duration() {
        let mut uptime = from_spans([(0, 10), (30, 40)]);
        uptime.clear();

        assert_eq!(uptime.active_duration(), 0);
    }

    #[test]
    fn extending_totals_the_same_as_building_at_once() {
        let mut piecemeal = from_spans([(0, 10)]);
        piecemeal.extend([span(5, 20), span(40, 50)]);

        assert_eq!(piecemeal.active_duration(), 30);
        assert_eq!(
            piecemeal.active_duration(),
            from_spans([(0, 20), (40, 50)]).active_duration(),
        );
    }

    #[test]
    fn the_operations_account_for_every_instant_they_move() {
        let a = from_spans([(0, 10), (20, 30)]);
        let b = from_spans([(5, 25)]);

        // The pieces of `a` partition `a`: what it shares with `b` plus what it
        // keeps to itself is everything it covered, with nothing double-counted.
        assert_eq!(
            a.intersection(&b).active_duration() + a.difference(&b).active_duration(),
            a.active_duration(),
        );

        // Inclusion-exclusion, the same statement from the union's side.
        assert_eq!(
            a.union(&b).active_duration() + a.intersection(&b).active_duration(),
            a.active_duration() + b.active_duration(),
        );

        // XOR is the union minus the shared part, counted once.
        assert_eq!(
            a.symmetric_difference(&b).active_duration(),
            a.union(&b).active_duration() - a.intersection(&b).active_duration(),
        );
    }

    #[test]
    fn duration_is_exact_where_a_single_span_would_saturate() {
        // `TimeIntervalEvent::duration` clamps this to `i64::MAX`; the sequence
        // must not, and `u64` is exactly wide enough to say so.
        let widest = from_spans([(i64::MIN, i64::MAX)]);
        assert_eq!(widest.active_duration(), u64::MAX);
        assert_eq!(widest.as_slice()[0].duration(), i64::MAX);

        // Carving the middle out leaves everything but those two ticks.
        let carved = widest.difference(&from_spans([(-1, 1)]));
        assert_eq!(bounds(&carved), [(i64::MIN, -1), (1, i64::MAX)]);
        assert_eq!(carved.active_duration(), u64::MAX - 2);

        // And putting it back restores the total, through the merge branch.
        let mut restored = carved;
        restored.insert(span(-1, 1));
        assert_eq!(restored, widest);
        assert_eq!(restored.active_duration(), u64::MAX);
    }

    #[test]
    fn transforming_by_nothing_changes_nothing() {
        let uptime = from_spans([(0, 10), (20, 30)]);

        assert_eq!(transformed(&uptime, 0, 0), uptime);
        assert!(transformed(&TimeIntervalSequence::new(), 3, -3).is_empty());
    }

    #[test]
    fn an_equal_shift_translates_every_instant() {
        let uptime = from_spans([(0, 10), (20, 30)]);
        let later = transformed(&uptime, 7, 7);

        assert_eq!(bounds(&later), [(7, 17), (27, 37)]);

        // Nothing widened or narrowed, so the covered instants only moved.
        assert_eq!(later.active_duration(), uptime.active_duration());
        for timestamp in -5..=105 {
            assert_eq!(later.contains(timestamp + 7), uptime.contains(timestamp));
        }
    }

    #[test]
    fn widening_merges_a_gap_it_closes_and_leaves_a_narrower_one_open() {
        // Two ticks of slack on each side close a four-tick gap to exactly
        // zero, and touching spans cover an unbroken stretch.
        assert_eq!(
            bounds(&transformed(&from_spans([(0, 10), (14, 20)]), -2, 2)),
            [(-2, 22)]
        );

        // One tick more of gap than the shift can close keeps them apart.
        assert_eq!(
            bounds(&transformed(&from_spans([(0, 10), (15, 20)]), -2, 2)),
            [(-2, 12), (13, 22)]
        );
    }

    #[test]
    fn narrowing_drops_spans_no_wider_than_the_shrinkage() {
        // Narrowing by three: a span of exactly three ticks has no interior
        // left, while one of four keeps a single tick.
        let uptime = from_spans([(0, 3), (10, 14), (20, 40)]);

        assert_eq!(bounds(&transformed(&uptime, 3, 0)), [(13, 14), (23, 40)]);
    }

    #[test]
    fn a_long_enough_widening_collapses_a_whole_chain() {
        let flapping = from_spans([(0, 1), (5, 6), (10, 11), (15, 16)]);

        assert_eq!(bounds(&transformed(&flapping, -3, 3)), [(-3, 19)]);
    }

    #[test]
    fn widening_only_merges_and_narrowing_only_drops() {
        let uptime = from_spans([(0, 10), (12, 22), (24, 25)]);

        // Widening keeps every instant it started from — it can merge spans,
        // but never removes one.
        let wider = transformed(&uptime, -2, 0);
        assert!(wider.len() <= uptime.len());
        for timestamp in -5..=105 {
            assert!(!uptime.contains(timestamp) || wider.contains(timestamp));
        }

        // Narrowing covers nothing new — it can drop spans, but never brings
        // two together, so every gap it started with is still there.
        let narrower = transformed(&uptime, 2, 0);
        assert!(narrower.len() <= uptime.len());
        for timestamp in -5..=105 {
            assert!(!narrower.contains(timestamp) || uptime.contains(timestamp));
        }
        assert_eq!(bounds(&narrower), [(2, 10), (14, 22)]);
    }

    #[test]
    fn transforms_add_up_until_one_of_them_loses_something() {
        let uptime = from_spans([(0, 10), (20, 30)]);

        // Nothing merges or vanishes along the way, so the shifts just sum.
        let stepwise = transformed(&transformed(&uptime, 1, 2), 3, -1);
        assert_eq!(stepwise, transformed(&uptime, 4, 1));

        // Once a widening has swallowed a gap, narrowing back cannot reopen it:
        // the two spans are one span now, and it simply narrows.
        let swallowed = transformed(&from_spans([(0, 10), (12, 22)]), -1, 1);
        assert_eq!(bounds(&swallowed), [(-1, 23)]);
        assert_eq!(bounds(&transformed(&swallowed, 1, -1)), [(0, 22)]);
    }

    #[test]
    fn a_shift_past_the_end_of_time_is_rejected() {
        let late = from_spans([(0, i64::MAX - 1)]);

        assert_eq!(
            late.transform(0, 2),
            Err(IntervalError::BoundOverflow {
                bound: i64::MAX - 1,
                shift: 2,
            })
        );

        let early = from_spans([(i64::MIN + 1, 0)]);
        assert_eq!(
            early.transform(-2, 0),
            Err(IntervalError::BoundOverflow {
                bound: i64::MIN + 1,
                shift: -2,
            })
        );

        // The receiver is borrowed, so a rejected transform costs it nothing.
        assert_eq!(bounds(&late), [(0, i64::MAX - 1)]);
        assert_eq!(bounds(&early), [(i64::MIN + 1, 0)]);
    }

    #[test]
    fn a_bound_landing_exactly_on_the_limit_is_accepted() {
        assert_eq!(
            bounds(&transformed(&from_spans([(0, i64::MAX - 1)]), 0, 1)),
            [(0, i64::MAX)]
        );
        assert_eq!(
            bounds(&transformed(&from_spans([(i64::MIN + 1, 0)]), -1, 0)),
            [(i64::MIN, 0)]
        );
    }

    #[test]
    fn equality_ignores_the_duration_and_stays_coverage_equality() {
        let piecemeal = from_spans([(0, 5), (5, 10), (2, 7)]);
        let whole = from_spans([(0, 10)]);

        assert_eq!(piecemeal, whole);
        assert_eq!(piecemeal.active_duration(), whole.active_duration());

        // Same span count and same total, but not the same instants.
        let shifted = from_spans([(0, 5), (10, 15)]);
        let elsewhere = from_spans([(20, 25), (30, 35)]);
        assert_eq!(shifted.active_duration(), elsewhere.active_duration());
        assert_ne!(shifted, elsewhere);
    }
}
