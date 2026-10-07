//! An always-ordered sequence of point events.

use crate::primitives::TimePointEvent;

mod accessors;
mod mutation;
mod queries;
mod traits;

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
/// When the events already exist, build with [`from_events`] rather than a loop
/// of [`insert`]: it sorts once, in place, instead of finding a home for each
/// event in turn. [`collect`][Iterator::collect] takes the same path.
///
/// [`from_events`]: TimePointSequence::from_events
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
    #[must_use]
    pub const fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Build a sequence from events already in hand.
    ///
    /// The usual way to create one when the data exists up front rather than
    /// arriving over time. The events may be in any order; they are sorted
    /// once, in place, reusing the `Vec` you pass rather than building a second
    /// one. Events sharing an instant keep the order they appear in.
    ///
    /// Already-ordered input costs only a scan to confirm it — the sort
    /// recognises a sorted run and does no work — so there is no reason to
    /// reach for anything else when the data comes out of a time-ordered file
    /// or query.
    ///
    /// Anything iterable works through [`collect`][Iterator::collect], which
    /// takes this same path:
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let readings: TimePointSequence<f64> = [
    ///     TimePointEvent::new(20, 2.0),
    ///     TimePointEvent::new(10, 1.0),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// assert_eq!(readings.first().map(|e| e.timestamp()), Some(10));
    /// ```
    #[must_use]
    pub fn from_events(mut events: Vec<TimePointEvent<T>>) -> Self {
        // Stable, so events sharing an instant keep their given order.
        events.sort_by_key(TimePointEvent::timestamp);

        Self { events }
    }
}
