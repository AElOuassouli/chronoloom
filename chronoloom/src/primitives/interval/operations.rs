//! Combining two intervals on the time dimension: where they overlap, and what
//! they cover together.
//!
//! All three work on the time dimension only, so none of them restates it: the
//! two intervals need not carry the same kind of value, neither value is
//! consumed, and the resulting spans carry none at all. Reattach one with
//! [`map`] where the result needs to mean something.
//!
//! [`map`]: TimeIntervalEvent::map

use super::TimeIntervalEvent;

impl<T> TimeIntervalEvent<T> {
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
