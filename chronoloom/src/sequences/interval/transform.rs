//! Moving every span's bounds, and the overflow check that guards it.

use crate::primitives::{IntervalError, TimeIntervalEvent, Timestamp};

use core::convert::Infallible;

use crate::sequences::extent::{shift_bound, shift_spans, Extent};

use super::normalization::absorb;
use super::operations::{difference_spans, union_spans};
use super::TimeIntervalSequence;

impl TimeIntervalSequence {
    /// The timeline with every span's bounds moved: `alpha` shifts the lower
    /// bound, `beta` the upper.
    ///
    /// Written `A[alpha, beta]` for a timeline `A`, this turns every span
    /// `[s, e)` into `[s + alpha, e + beta)`. Both shifts may be negative, and
    /// both move the bound to the right when positive. `A[0, 0]` is `A`, and
    /// `A[t, t]` is `A` translated by `t`. A single linear pass, with no
    /// sorting: shifting every bound by the same amount leaves the spans in the
    /// order they were already in.
    ///
    /// Because the two bounds move independently, the result is re-normalized
    /// rather than merely re-bounded — one call can merge spans or drop them,
    /// but never both. See [the type documentation][transforming] for which,
    /// why, and how the result describes itself.
    ///
    /// Widening merges the spans it brings together — and a gap closed to
    /// exactly zero counts, since touching spans cover an unbroken stretch:
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let alerts = TimeIntervalSequence::from_spans("alerts", vec![
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
    /// let up = TimeIntervalSequence::from_spans("up", vec![
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
    /// # Errors
    ///
    /// Returns [`IntervalError::BoundOverflow`] if any shifted bound leaves the
    /// range a [`Timestamp`] can hold, or if the shifts themselves total more
    /// than one can hold. Nothing is transformed in that case; the receiver is
    /// borrowed and never changes either way.
    ///
    /// [transforming]: TimeIntervalSequence#transforming-a-timeline
    pub fn transform(&self, alpha: Timestamp, beta: Timestamp) -> Result<Self, IntervalError> {
        self.transform_by(alpha, beta, shifted)
    }

    /// The transform with bounds at the limits of the timestamp range read as
    /// "without end": they stay put, and every other bound saturates, as an
    /// extent's do. Formulas shift timelines whose open spans reach the end of
    /// time, so they use this and cannot fail.
    pub(crate) fn transform_unbounded(&self, alpha: Timestamp, beta: Timestamp) -> Self {
        let Ok(transformed) = self.transform_by(alpha, beta, |bound, shift| {
            Ok::<_, Infallible>(shift_bound(bound, shift))
        });
        transformed
    }

    /// The transform, with `shift` moving each bound.
    fn transform_by<E>(
        &self,
        alpha: Timestamp,
        beta: Timestamp,
        shift: impl Fn(Timestamp, Timestamp) -> Result<Timestamp, E>,
    ) -> Result<Self, E> {
        // Before anything moves, so a failure leaves nothing half-done. The
        // running total needs checking in its own right: an empty timeline has
        // no bounds for the loop below to reject a runaway shift on.
        let transformation = (
            shift(self.transformation.0, alpha)?,
            shift(self.transformation.1, beta)?,
        );

        // Merging only ever removes spans, so this capacity is an upper bound.
        let mut spans = Vec::with_capacity(self.spans.len());

        for span in &self.spans {
            let (start, end) = span.bounds();
            let start = shift(start, alpha)?;
            let end = shift(end, beta)?;

            // The bounds crossed, so the span narrowed away to nothing.
            if start < end {
                // Both bounds moved by a fixed amount, so the shifted spans
                // arrive in the order they were stored in and the tail is the
                // only one a new span can reach.
                absorb(&mut spans, TimeIntervalEvent::raw(start, end));
            }
        }

        // Widening, an instant is known once its window holds an active instant
        // or lies wholly inside the extent; narrowing, once its window is
        // wholly active or holds an instant known inactive.
        let known = if alpha <= beta {
            shift_spans(self.observed.as_slice(), beta, alpha)
        } else {
            let inactive = difference_spans(self.observed.as_slice(), &self.spans);
            shift_spans(&inactive, beta, alpha)
        };
        let observed = Extent::from_normalized(union_spans(&known, &spans));

        Ok(Self::from_parts(
            self.attribute.clone(),
            transformation,
            spans,
            observed,
        ))
    }
}

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
