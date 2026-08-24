//! The interval event primitive: a value attached to a span of time.

use core::fmt;

use super::Timestamp;

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
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// assert_eq!(TimeIntervalEvent::new(3, 9, ())?.start(), 3);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub const fn start(&self) -> Timestamp {
        self.start
    }

    /// The first instant *past* the span, which the span does not cover.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// assert_eq!(TimeIntervalEvent::new(3, 9, ())?.end(), 9);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub const fn end(&self) -> Timestamp {
        self.end
    }

    /// The span as a `(start, end)` pair.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// assert_eq!(TimeIntervalEvent::new(3, 9, "run")?.bounds(), (3, 9));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
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
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let phase = TimeIntervalEvent::new(0, 60, String::from("warm-up"))?;
    /// assert_eq!(phase.value(), "warm-up");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consume the interval and return its value, dropping the bounds.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let phase = TimeIntervalEvent::new(0, 60, vec![1, 2, 3])?;
    /// assert_eq!(phase.into_value(), vec![1, 2, 3]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// Consume the interval and return its bounds and value.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let parts = TimeIntervalEvent::new(0, 60, 'x')?.into_parts();
    /// assert_eq!(parts, (0, 60, 'x'));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn into_parts(self) -> (Timestamp, Timestamp, T) {
        (self.start, self.end, self.value)
    }

    /// Transform the payload, keeping the same span.
    ///
    /// Cannot fail: the bounds are already valid and are left untouched.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let raw = TimeIntervalEvent::new(0, 60, 3_i32)?;
    /// let labelled = raw.map(|v| format!("phase {v}"));
    ///
    /// assert_eq!(labelled.bounds(), (0, 60));
    /// assert_eq!(labelled.value(), "phase 3");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
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
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let downtime = TimeIntervalEvent::span(120, 180)?;
    ///
    /// assert_eq!(downtime.duration(), 60);
    /// assert!(TimeIntervalEvent::span(180, 120).is_err());
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`new`]: TimeIntervalEvent::new
    pub fn span(start: Timestamp, end: Timestamp) -> Result<Self, IntervalError> {
        Self::new(start, end, ())
    }
}

/// Why a [`TimeIntervalEvent`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IntervalError {
    /// `end` was not strictly after `start`, so the span would be empty or
    /// inverted.
    ///
    /// ```
    /// use chronoloom::primitives::{IntervalError, TimeIntervalEvent};
    ///
    /// let error = TimeIntervalEvent::span(9, 0).unwrap_err();
    /// assert_eq!(error, IntervalError::EndNotAfterStart { start: 9, end: 0 });
    /// ```
    EndNotAfterStart {
        /// The rejected start bound.
        start: Timestamp,
        /// The rejected end bound.
        end: Timestamp,
    },
}

impl fmt::Display for IntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndNotAfterStart { start, end } => write!(
                f,
                "interval end ({end}) must be strictly after its start ({start})"
            ),
        }
    }
}

impl std::error::Error for IntervalError {}

#[cfg(test)]
mod tests {
    use super::{IntervalError, TimeIntervalEvent};
    use std::collections::BTreeSet;

    #[test]
    fn new_exposes_bounds_and_value() {
        let interval = TimeIntervalEvent::new(3, 9, 'x').expect("3 < 9");

        assert_eq!(interval.start(), 3);
        assert_eq!(interval.end(), 9);
        assert_eq!(interval.bounds(), (3, 9));
        assert_eq!(*interval.value(), 'x');
    }

    #[test]
    fn empty_intervals_are_rejected() {
        assert_eq!(
            TimeIntervalEvent::new(5, 5, ()),
            Err(IntervalError::EndNotAfterStart { start: 5, end: 5 })
        );
    }

    #[test]
    fn inverted_intervals_are_rejected() {
        assert_eq!(
            TimeIntervalEvent::new(9, 0, ()),
            Err(IntervalError::EndNotAfterStart { start: 9, end: 0 })
        );
    }

    #[test]
    fn negative_bounds_are_accepted() {
        let interval = TimeIntervalEvent::span(-10, -4).expect("-10 < -4");

        assert_eq!(interval.bounds(), (-10, -4));
        assert_eq!(interval.duration(), 6);
    }

    #[test]
    fn span_builds_a_valueless_interval() {
        let interval = TimeIntervalEvent::span(120, 180).expect("120 < 180");

        assert_eq!(interval.bounds(), (120, 180));
        assert_eq!(interval.into_parts(), (120, 180, ()));
        assert!(TimeIntervalEvent::span(180, 120).is_err());
    }

    #[test]
    fn duration_saturates_at_the_extremes() {
        let widest = TimeIntervalEvent::span(i64::MIN, i64::MAX).expect("MIN < MAX");

        assert_eq!(widest.duration(), i64::MAX);
    }

    #[test]
    fn payload_may_be_a_collection() {
        let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
        let interval = TimeIntervalEvent::new(0, 1, tags.clone()).expect("0 < 1");

        assert_eq!(interval.value(), &tags);
    }

    #[test]
    fn into_parts_round_trips_the_constructor_arguments() {
        let interval = TimeIntervalEvent::new(0, 60, String::from("warm-up")).expect("0 < 60");

        assert_eq!(interval.into_parts(), (0, 60, String::from("warm-up")));
    }

    #[test]
    fn map_transforms_the_value_and_preserves_the_bounds() {
        let interval = TimeIntervalEvent::new(0, 60, 3_i32).expect("0 < 60");
        let mapped = interval.map(|v| v.to_string());

        assert_eq!(mapped.bounds(), (0, 60));
        assert_eq!(mapped.value(), "3");
    }

    #[test]
    fn intervals_compare_on_every_field() {
        let interval = TimeIntervalEvent::new(0, 5, 'a').expect("0 < 5");

        assert_eq!(interval, TimeIntervalEvent::new(0, 5, 'a').expect("0 < 5"));
        assert_ne!(interval, TimeIntervalEvent::new(0, 6, 'a').expect("0 < 6"));
        assert_ne!(interval, TimeIntervalEvent::new(0, 5, 'b').expect("0 < 5"));
    }

    #[test]
    fn error_displays_both_bounds() {
        let error = TimeIntervalEvent::span(9, 0).unwrap_err();

        assert_eq!(
            error.to_string(),
            "interval end (0) must be strictly after its start (9)"
        );
    }

    #[test]
    fn error_is_a_standard_error() {
        let error: Box<dyn std::error::Error> =
            Box::new(TimeIntervalEvent::span(5, 5).unwrap_err());

        assert!(error.to_string().contains("strictly after"));
    }
}
