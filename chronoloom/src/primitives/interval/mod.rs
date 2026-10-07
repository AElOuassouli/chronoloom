//! The interval event primitive: a value attached to a span of time.

use crate::primitives::Timestamp;

mod errors;
mod operations;

pub use self::errors::IntervalError;

/// A value attached to a half-open span of time `[start, end)`.
///
/// `start` is included and `end` is excluded, so two intervals that merely
/// touch — `[0, 5)` and `[5, 9)` — share no instant. The span is always
/// non-empty: [`new`] rejects anything where `end` is not strictly after
/// `start`.
///
/// The payload is generic and unconstrained, so an interval can carry a label,
/// a measurement, a collection, or nothing at all — see [`span`] for the
/// valueless case.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
///
/// let phase = TimeIntervalEvent::new(0, 60, "warm-up")?;
///
/// assert_eq!(phase.start(), 0);
/// assert_eq!(phase.end(), 60);
/// assert_eq!(phase.value(), &"warm-up");
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// Ordering is deliberately not derived: comparing intervals would otherwise
/// fold the payload into the comparison, and there is more than one defensible
/// order over spans. Sort on an explicit key instead.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
///
/// let mut phases = vec![
///     TimeIntervalEvent::new(10, 20, 'b')?,
///     TimeIntervalEvent::new(0, 10, 'a')?,
/// ];
/// phases.sort_by_key(TimeIntervalEvent::start);
///
/// assert_eq!(*phases[0].value(), 'a');
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// [`new`]: TimeIntervalEvent::new
/// [`span`]: TimeIntervalEvent::span
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeIntervalEvent<T> {
    start: Timestamp,
    end: Timestamp,
    value: T,
}

impl<T> TimeIntervalEvent<T> {
    /// Attach `value` to the half-open span `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns [`IntervalError::EndNotAfterStart`] unless `start < end`. Both
    /// inverted spans and empty ones are rejected, so every
    /// `TimeIntervalEvent` covers at least one instant.
    ///
    /// ```
    /// use chronoloom::primitives::{IntervalError, TimeIntervalEvent};
    ///
    /// assert!(TimeIntervalEvent::new(0, 5, ()).is_ok());
    ///
    /// assert_eq!(
    ///     TimeIntervalEvent::new(5, 5, ()),
    ///     Err(IntervalError::EndNotAfterStart { start: 5, end: 5 }),
    /// );
    /// assert_eq!(
    ///     TimeIntervalEvent::new(9, 0, ()),
    ///     Err(IntervalError::EndNotAfterStart { start: 9, end: 0 }),
    /// );
    /// ```
    pub fn new(start: Timestamp, end: Timestamp, value: T) -> Result<Self, IntervalError> {
        if start < end {
            Ok(Self { start, end, value })
        } else {
            Err(IntervalError::EndNotAfterStart { start, end })
        }
    }

    /// The first instant covered by the span.
    #[must_use]
    pub const fn start(&self) -> Timestamp {
        self.start
    }

    /// The first instant *past* the span, which the span does not cover.
    #[must_use]
    pub const fn end(&self) -> Timestamp {
        self.end
    }

    /// The span as a `(start, end)` pair.
    #[must_use]
    pub const fn bounds(&self) -> (Timestamp, Timestamp) {
        (self.start, self.end)
    }

    /// How long the span lasts, in the same ticks as its bounds.
    ///
    /// Always strictly positive, since an interval cannot be empty. The
    /// subtraction saturates, so a span covering nearly the whole `i64` range
    /// reports [`i64::MAX`] rather than overflowing.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// assert_eq!(TimeIntervalEvent::new(3, 9, ())?.duration(), 6);
    /// assert_eq!(
    ///     TimeIntervalEvent::new(i64::MIN, i64::MAX, ())?.duration(),
    ///     i64::MAX,
    /// );
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub const fn duration(&self) -> i64 {
        self.end.saturating_sub(self.start)
    }

    /// The value carried by this interval.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consume the interval and return its value, dropping the bounds.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// Consume the interval and return its bounds and value.
    #[must_use]
    pub fn into_parts(self) -> (Timestamp, Timestamp, T) {
        (self.start, self.end, self.value)
    }

    /// Transform the payload, keeping the same span.
    ///
    /// Cannot fail: the bounds are already valid and are left untouched.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> TimeIntervalEvent<U> {
        TimeIntervalEvent {
            start: self.start,
            end: self.end,
            value: f(self.value),
        }
    }
}

impl TimeIntervalEvent<()> {
    /// Build a bare span `[start, end)` that carries no value.
    ///
    /// A shorthand for `TimeIntervalEvent::new(start, end, ())`.
    ///
    /// # Errors
    ///
    /// Returns [`IntervalError::EndNotAfterStart`] unless `start < end`, just
    /// like [`new`].
    ///
    /// [`new`]: TimeIntervalEvent::new
    pub fn span(start: Timestamp, end: Timestamp) -> Result<Self, IntervalError> {
        Self::new(start, end, ())
    }

    /// Build a span whose bounds are already known to be valid.
    ///
    /// Crate-internal and unvalidated: the caller must have proven
    /// `start < end`, otherwise the non-empty invariant is broken. Every use
    /// derives its bounds from intervals that already uphold it — the interval
    /// operations here, and the merging in [`sequences`]. Validating again
    /// would be dead code, and an `expect` would add a panic path that can
    /// never fire.
    ///
    /// [`sequences`]: crate::sequences
    pub(crate) const fn raw(start: Timestamp, end: Timestamp) -> Self {
        Self {
            start,
            end,
            value: (),
        }
    }
}
