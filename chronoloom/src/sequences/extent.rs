//! The instants over which a sequence was observed.

use core::ops::{Bound, RangeBounds};

use crate::primitives::{TimeIntervalEvent, Timestamp};

use super::interval::normalization::{absorb, normalize};
use super::interval::operations::{intersection_spans, union_spans};

/// The instants over which a sequence was observed, as normalized spans.
///
/// A timeline with no span after `t` reads as "inactive after `t`", whether the
/// dataset stopped, the sensor was offline, or the data has not arrived yet.
/// The extent says which: inside it the sequence speaks, so an instant with no
/// span is known inactive; outside it the sequence says nothing, so the instant
/// is *unknown*. Every [`TimeIntervalSequence`] and [`TimePointSequence`]
/// carries one, and `is_observed` tells inactive from unknown, where `contains`
/// and `at` keep answering only whether the instant is active.
///
/// The extent is kept like a timeline: sorted, disjoint, and never touching.
/// It starts as [`Extent::everything`], so a sequence that never mentions its
/// extent behaves as though nothing is unknown. The instant [`i64::MAX`] is
/// outside it, since the spans are half-open, and a point event stamped there
/// is kept although no extent covers it, and so is the end edge that
/// [`Events::Edges`] puts on the exclusive end of a span reaching the end of
/// the extent.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::{Extent, TimeIntervalSequence};
///
/// let stopped_at_ten = Extent::from_spans(vec![TimeIntervalEvent::span(0, 10)?]);
/// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(2, 5)?])
///     .with_observed(stopped_at_ten);
///
/// // Inactive and unknown both read as "not active", and only one is known.
/// assert!(!up.contains(7) && up.is_observed(7));
/// assert!(!up.contains(20) && !up.is_observed(20));
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// # Data stays inside the extent
///
/// A sequence never holds data outside its extent. `with_observed` sets the
/// extent and cuts the data to it, spans clipped and points dropped, while
/// inserting an event outside it **grows** the extent to cover it, because an
/// event is evidence that its instant was observed.
///
/// # Three-valued operations
///
/// Each instant of a timeline is active, inactive or unknown, and an operation
/// keeps its result wherever it can be decided from what is known. A union is
/// active wherever either side is known active, even where the other side is
/// unknown, and an intersection is inactive wherever either side is known
/// inactive. So for operands `a` and `b` observed over `Ea` and `Eb`, the result
/// is observed over:
///
/// | Operation | Observed over |
/// |---|---|
/// | `a ∪ b` | `(Ea ∩ Eb) ∪ a ∪ b` |
/// | `a ∩ b` | `(Ea ∩ Eb) ∪ (Ea \ a) ∪ (Eb \ b)` |
/// | `a \ b` | `(Ea ∩ Eb) ∪ (Ea \ a) ∪ b` |
/// | `a △ b` | `Ea ∩ Eb` |
/// | `a.complement()` | `Ea`, and it is active on `Ea \ a` |
///
/// `transform(alpha, beta)` is decided from a window of the input around each
/// instant. A widening transform is active as soon as the window holds an
/// active instant, and known inactive only when all of it is observed. A
/// narrowing one is active only when the whole window is active, and known
/// inactive as soon as the window holds an instant known inactive. Bounds at
/// `i64::MIN` and `i64::MAX` stay where they are when an extent is shifted, so
/// an extent of everything stays everything.
///
/// [`Events::Edges`]: crate::processing::Events::Edges
/// [`TimeIntervalSequence`]: crate::sequences::TimeIntervalSequence
/// [`TimePointSequence`]: crate::sequences::TimePointSequence
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extent {
    /// Normalized: sorted, disjoint, and never touching.
    spans: Vec<TimeIntervalEvent<()>>,
}

impl Extent {
    /// Every instant, as `[i64::MIN, i64::MAX)`: nothing is unknown.
    #[must_use]
    pub fn everything() -> Self {
        Self {
            spans: vec![TimeIntervalEvent::raw(Timestamp::MIN, Timestamp::MAX)],
        }
    }

    /// No instant at all: everything is unknown.
    #[must_use]
    pub const fn nothing() -> Self {
        Self { spans: Vec::new() }
    }

    /// The instants covered by `spans`, given in any order and merged as a
    /// timeline merges them.
    #[must_use]
    pub fn from_spans(spans: Vec<TimeIntervalEvent<()>>) -> Self {
        Self::from_normalized(normalize(spans))
    }

    /// Whether `timestamp` is observed.
    #[must_use]
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        self.covers(timestamp, timestamp.saturating_add(1)) && timestamp < Timestamp::MAX
    }

    /// The spans of the extent, in time order.
    #[must_use]
    pub fn as_slice(&self) -> &[TimeIntervalEvent<()>] {
        &self.spans
    }

    /// The instants of `range`, as an extent.
    ///
    /// Unbounded sides reach [`Timestamp::MIN`] and [`Timestamp::MAX`], as in
    /// [`Extent::everything`]. The instant `i64::MAX` is outside every extent,
    /// so `..=i64::MAX` covers what `..` does. A range with no instant in it,
    /// reversed ones included, gives [`Extent::nothing`].
    pub(crate) fn of_range(range: &impl RangeBounds<Timestamp>) -> Self {
        let start = match range.start_bound() {
            Bound::Included(&timestamp) => timestamp,
            Bound::Excluded(&timestamp) => timestamp.saturating_add(1),
            Bound::Unbounded => Timestamp::MIN,
        };
        let end = match range.end_bound() {
            Bound::Included(&timestamp) => timestamp.saturating_add(1),
            Bound::Excluded(&timestamp) => timestamp,
            Bound::Unbounded => Timestamp::MAX,
        };

        if start < end {
            Self::from_normalized(vec![TimeIntervalEvent::raw(start, end)])
        } else {
            Self::nothing()
        }
    }

    /// Wrap spans that are already normalized.
    pub(crate) const fn from_normalized(spans: Vec<TimeIntervalEvent<()>>) -> Self {
        Self { spans }
    }

    /// The instants observed by both extents.
    pub(crate) fn intersection_spans(&self, other: &Self) -> Vec<TimeIntervalEvent<()>> {
        intersection_spans(&self.spans, &other.spans)
    }

    /// Narrow to the instants `other` also observes.
    pub(crate) fn narrow(&mut self, other: &Self) {
        self.spans = self.intersection_spans(other);
    }

    /// Whether the span `[start, end)` lies inside one span of the extent.
    pub(crate) fn covers(&self, start: Timestamp, end: Timestamp) -> bool {
        self.spans
            .partition_point(|span| span.start() <= start)
            .checked_sub(1)
            .and_then(|index| self.spans.get(index))
            .is_some_and(|span| span.end() >= end)
    }

    /// Extend the extent to cover `spans`, which must be normalized.
    pub(crate) fn grow(&mut self, spans: &[TimeIntervalEvent<()>]) {
        if !spans
            .iter()
            .all(|span| self.covers(span.start(), span.end()))
        {
            self.spans = union_spans(&self.spans, spans);
        }
    }
}

impl Default for Extent {
    /// Everything, as [`Extent::everything`].
    fn default() -> Self {
        Self::everything()
    }
}

/// Move each run `[s, e)` of `spans` to `[s + lower, e + upper)`, dropping the
/// runs that nothing is left of.
///
/// Meant for extents rather than timelines: bounds at the limits of the
/// timestamp range stand for "without end", so they stay put, and any other
/// bound saturates rather than overflows.
pub(crate) fn shift_spans(
    spans: &[TimeIntervalEvent<()>],
    lower: Timestamp,
    upper: Timestamp,
) -> Vec<TimeIntervalEvent<()>> {
    let mut shifted: Vec<TimeIntervalEvent<()>> = Vec::with_capacity(spans.len());

    for span in spans {
        let start = shift_bound(span.start(), lower);
        let end = shift_bound(span.end(), upper);
        if start < end {
            // Both bounds move by a fixed amount, so the runs stay in order.
            absorb(&mut shifted, TimeIntervalEvent::raw(start, end));
        }
    }

    shifted
}

/// Move `bound` by `shift`, clamped to the timestamp range, except that the
/// limits of the range stay where they are.
pub(crate) fn shift_bound(bound: Timestamp, shift: Timestamp) -> Timestamp {
    if bound == Timestamp::MIN || bound == Timestamp::MAX {
        bound
    } else {
        bound.saturating_add(shift)
    }
}
