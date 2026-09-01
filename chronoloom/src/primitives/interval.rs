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

    /// The span where both intervals are active, or `None` where they never
    /// are.
    ///
    /// Because intervals are half-open, two that merely touch share no instant
    /// and so do not intersect — `[0, 5)` ends just before `[5, 9)` begins.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let a = TimeIntervalEvent::new(0, 5, "a")?;
    /// let b = TimeIntervalEvent::new(3, 9, "b")?;
    ///
    /// assert_eq!(a.intersection(&b), Some(TimeIntervalEvent::span(3, 5)?));
    ///
    /// let touching = TimeIntervalEvent::new(5, 9, "c")?;
    /// assert_eq!(a.intersection(&touching), None);
    ///
    /// let apart = TimeIntervalEvent::new(20, 30, "d")?;
    /// assert_eq!(a.intersection(&apart), None);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// The two intervals need not carry the same kind of value, and neither
    /// value is consumed. The result carries no value at all — reattach one
    /// with [`map`] if the overlap needs to mean something.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let measured = TimeIntervalEvent::new(0, 5, 21.5_f64)?;
    /// let labelled = TimeIntervalEvent::new(3, 9, String::from("warm-up"))?;
    ///
    /// let both = measured.intersection(&labelled).unwrap();
    /// assert_eq!(both.map(|()| "overlap").value(), &"overlap");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`map`]: TimeIntervalEvent::map
    #[must_use]
    pub fn intersection<U>(&self, other: &TimeIntervalEvent<U>) -> Option<TimeIntervalEvent<()>> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);

        (start < end).then(|| TimeIntervalEvent::raw(start, end))
    }

    /// The single span covering both intervals, or `None` when a gap separates
    /// them.
    ///
    /// Intervals combine when they overlap **and** when they merely touch,
    /// since `[0, 5)` and `[5, 9)` together cover exactly `[0, 9)` with no
    /// instant missing. Only a real gap keeps them apart, and then there is no
    /// single span to describe them.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let a = TimeIntervalEvent::new(0, 5, "a")?;
    ///
    /// let overlapping = TimeIntervalEvent::new(3, 9, "b")?;
    /// assert_eq!(a.merged(&overlapping), Some(TimeIntervalEvent::span(0, 9)?));
    ///
    /// let touching = TimeIntervalEvent::new(5, 9, "c")?;
    /// assert_eq!(a.merged(&touching), Some(TimeIntervalEvent::span(0, 9)?));
    ///
    /// let apart = TimeIntervalEvent::new(20, 30, "d")?;
    /// assert_eq!(a.merged(&apart), None);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// This is the counterpart to [`intersection`]: one answers what the two
    /// intervals share, the other what they cover together. [`union`] answers
    /// unconditionally by returning both intervals when they do not combine.
    ///
    /// [`intersection`]: TimeIntervalEvent::intersection
    /// [`union`]: TimeIntervalEvent::union
    #[must_use]
    pub fn merged<U>(&self, other: &TimeIntervalEvent<U>) -> Option<TimeIntervalEvent<()>> {
        // Non-strict on both sides, which is what lets merely touching
        // intervals combine rather than stay apart.
        let combines = self.start <= other.end && other.start <= self.end;

        combines
            .then(|| TimeIntervalEvent::raw(self.start.min(other.start), self.end.max(other.end)))
    }

    /// The spans covered by either interval: one when they combine, two when
    /// they stay apart.
    ///
    /// Intervals combine when they overlap **and** when they merely touch,
    /// since `[0, 5)` and `[5, 9)` together cover exactly `[0, 9)` with no
    /// instant missing. Only a real gap keeps them separate. Use [`merged`]
    /// instead when a gap should simply answer "no single span", without
    /// allocating.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let a = TimeIntervalEvent::new(0, 5, "a")?;
    ///
    /// let overlapping = TimeIntervalEvent::new(3, 9, "b")?;
    /// assert_eq!(a.union(&overlapping), vec![TimeIntervalEvent::span(0, 9)?]);
    ///
    /// let touching = TimeIntervalEvent::new(5, 9, "c")?;
    /// assert_eq!(a.union(&touching), vec![TimeIntervalEvent::span(0, 9)?]);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// Intervals separated by a gap come back unmerged, always ordered by
    /// start, whichever one the method was called on.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    ///
    /// let early = TimeIntervalEvent::new(0, 2, "a")?;
    /// let late = TimeIntervalEvent::new(5, 9, "b")?;
    /// let apart = vec![
    ///     TimeIntervalEvent::span(0, 2)?,
    ///     TimeIntervalEvent::span(5, 9)?,
    /// ];
    ///
    /// assert_eq!(early.union(&late), apart);
    /// assert_eq!(late.union(&early), apart);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// As with [`intersection`], the two intervals need not carry the same kind
    /// of value, neither value is consumed, and the resulting spans carry none.
    ///
    /// [`intersection`]: TimeIntervalEvent::intersection
    /// [`merged`]: TimeIntervalEvent::merged
    #[must_use]
    pub fn union<U>(&self, other: &TimeIntervalEvent<U>) -> Vec<TimeIntervalEvent<()>> {
        // Whether they combine, and into what, is defined once — in `merged`.
        if let Some(merged) = self.merged(other) {
            return vec![merged];
        }

        let mine = TimeIntervalEvent::raw(self.start, self.end);
        let theirs = TimeIntervalEvent::raw(other.start, other.end);

        if self.start <= other.start {
            vec![mine, theirs]
        } else {
            vec![theirs, mine]
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

/// Why a [`TimeIntervalEvent`] could not be built, or a bound could not be
/// moved.
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

    /// Moving `bound` by `shift` landed outside the range a [`Timestamp`] can
    /// hold.
    ///
    /// Raised by [`TimeIntervalSequence::transform`], the one operation whose
    /// arithmetic is driven by caller-supplied numbers rather than by bounds
    /// that already exist.
    ///
    /// ```
    /// use chronoloom::primitives::{IntervalError, TimeIntervalEvent};
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let late = TimeIntervalSequence::from_spans(vec![
    ///     TimeIntervalEvent::span(0, i64::MAX - 1)?,
    /// ]);
    ///
    /// let error = late.transform(0, 2).unwrap_err();
    /// assert_eq!(
    ///     error,
    ///     IntervalError::BoundOverflow {
    ///         bound: i64::MAX - 1,
    ///         shift: 2,
    ///     }
    /// );
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`TimeIntervalSequence::transform`]: crate::sequences::TimeIntervalSequence::transform
    BoundOverflow {
        /// The bound that could not be moved.
        bound: Timestamp,
        /// How far it was asked to move.
        shift: Timestamp,
    },
}

impl fmt::Display for IntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndNotAfterStart { start, end } => write!(
                f,
                "interval end ({end}) must be strictly after its start ({start})"
            ),
            Self::BoundOverflow { bound, shift } => write!(
                f,
                "shifting bound ({bound}) by ({shift}) leaves the timestamp range"
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
    fn overflow_error_displays_the_bound_and_the_shift() {
        let error = IntervalError::BoundOverflow {
            bound: i64::MAX,
            shift: 1,
        };

        assert_eq!(
            error.to_string(),
            format!(
                "shifting bound ({}) by (1) leaves the timestamp range",
                i64::MAX
            )
        );
    }

    /// Shorthand for the valueless spans both operations return.
    fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
        TimeIntervalEvent::span(start, end).expect("test bounds are ordered")
    }

    #[test]
    fn overlapping_intervals_intersect_on_the_shared_span() {
        let a = span(0, 5);
        let b = span(3, 9);

        assert_eq!(a.intersection(&b), Some(span(3, 5)));
        assert_eq!(b.intersection(&a), Some(span(3, 5)));
    }

    #[test]
    fn a_contained_interval_intersects_to_itself() {
        let outer = span(0, 10);
        let inner = span(3, 5);

        assert_eq!(outer.intersection(&inner), Some(span(3, 5)));
        assert_eq!(inner.intersection(&outer), Some(span(3, 5)));
    }

    #[test]
    fn an_interval_intersects_itself() {
        let a = span(0, 5);

        assert_eq!(a.intersection(&a), Some(span(0, 5)));
    }

    #[test]
    fn touching_intervals_do_not_intersect() {
        let a = span(0, 5);
        let b = span(5, 9);

        assert_eq!(a.intersection(&b), None);
        assert_eq!(b.intersection(&a), None);
    }

    #[test]
    fn disjoint_intervals_do_not_intersect() {
        let a = span(0, 2);
        let b = span(5, 9);

        assert_eq!(a.intersection(&b), None);
        assert_eq!(b.intersection(&a), None);
    }

    #[test]
    fn intersection_handles_negative_bounds() {
        let a = span(-10, -2);
        let b = span(-5, 5);

        assert_eq!(a.intersection(&b), Some(span(-5, -2)));
    }

    #[test]
    fn overlapping_intervals_merge_into_one_span() {
        assert_eq!(span(0, 5).merged(&span(3, 9)), Some(span(0, 9)));
        assert_eq!(span(3, 9).merged(&span(0, 5)), Some(span(0, 9)));
    }

    #[test]
    fn touching_intervals_merge_into_one_span() {
        assert_eq!(span(0, 5).merged(&span(5, 9)), Some(span(0, 9)));
        assert_eq!(span(5, 9).merged(&span(0, 5)), Some(span(0, 9)));
    }

    #[test]
    fn a_contained_interval_merges_into_the_outer_one() {
        assert_eq!(span(0, 10).merged(&span(3, 5)), Some(span(0, 10)));
        assert_eq!(span(3, 5).merged(&span(0, 10)), Some(span(0, 10)));
    }

    #[test]
    fn an_interval_merges_with_itself_into_itself() {
        assert_eq!(span(0, 5).merged(&span(0, 5)), Some(span(0, 5)));
    }

    #[test]
    fn intervals_separated_by_a_gap_do_not_merge() {
        assert_eq!(span(0, 2).merged(&span(5, 9)), None);
        assert_eq!(span(5, 9).merged(&span(0, 2)), None);
    }

    #[test]
    fn merged_handles_negative_bounds() {
        assert_eq!(span(-10, -2).merged(&span(-5, 5)), Some(span(-10, 5)));
    }

    #[test]
    fn merged_agrees_with_union() {
        // `union` is written on top of `merged`; this pins that they stay
        // consistent rather than drifting into two rules.
        for (a, b) in [
            (span(0, 5), span(3, 9)),
            (span(0, 5), span(5, 9)),
            (span(0, 2), span(5, 9)),
            (span(0, 10), span(3, 5)),
        ] {
            match a.merged(&b) {
                Some(merged) => assert_eq!(a.union(&b), vec![merged]),
                None => assert_eq!(a.union(&b).len(), 2),
            }
        }
    }

    #[test]
    fn overlapping_intervals_unite_into_one_span() {
        let a = span(0, 5);
        let b = span(3, 9);

        assert_eq!(a.union(&b), vec![span(0, 9)]);
        assert_eq!(b.union(&a), vec![span(0, 9)]);
    }

    #[test]
    fn touching_intervals_unite_into_one_span() {
        let a = span(0, 5);
        let b = span(5, 9);

        assert_eq!(a.union(&b), vec![span(0, 9)]);
        assert_eq!(b.union(&a), vec![span(0, 9)]);
    }

    #[test]
    fn a_contained_interval_unites_into_the_outer_span() {
        let outer = span(0, 10);
        let inner = span(3, 5);

        assert_eq!(outer.union(&inner), vec![span(0, 10)]);
        assert_eq!(inner.union(&outer), vec![span(0, 10)]);
    }

    #[test]
    fn an_interval_unites_with_itself_into_itself() {
        let a = span(0, 5);

        assert_eq!(a.union(&a), vec![span(0, 5)]);
    }

    #[test]
    fn disjoint_intervals_stay_apart_ordered_by_start() {
        let early = span(0, 2);
        let late = span(5, 9);
        let apart = vec![span(0, 2), span(5, 9)];

        assert_eq!(early.union(&late), apart);
        assert_eq!(late.union(&early), apart);
    }

    #[test]
    fn union_yields_one_span_or_two_and_never_more() {
        let a = span(0, 5);

        assert_eq!(a.union(&span(3, 9)).len(), 1);
        assert_eq!(a.union(&span(5, 9)).len(), 1);
        assert_eq!(a.union(&span(20, 30)).len(), 2);
    }

    #[test]
    fn union_handles_negative_bounds() {
        let a = span(-10, -5);
        let b = span(-5, 0);

        assert_eq!(a.union(&b), vec![span(-10, 0)]);
    }

    #[test]
    fn operations_combine_intervals_carrying_different_value_types() {
        let measured = TimeIntervalEvent::new(0, 5, 21.5_f64).expect("0 < 5");
        let labelled = TimeIntervalEvent::new(3, 9, String::from("warm-up")).expect("3 < 9");

        assert_eq!(measured.intersection(&labelled), Some(span(3, 5)));
        assert_eq!(measured.union(&labelled), vec![span(0, 9)]);

        assert_eq!(measured.value(), &21.5);
        assert_eq!(labelled.value(), "warm-up");
    }

    #[test]
    fn error_is_a_standard_error() {
        let error: Box<dyn std::error::Error> =
            Box::new(TimeIntervalEvent::span(5, 5).unwrap_err());

        assert!(error.to_string().contains("strictly after"));
    }
}
