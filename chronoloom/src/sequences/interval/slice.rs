//! Cutting a timeline to a range of time.

use std::ops::RangeBounds;

use crate::primitives::Timestamp;
use crate::sequences::extent::Extent;

use super::operations::intersection_spans;
use super::TimeIntervalSequence;

impl TimeIntervalSequence {
    /// The part of the timeline inside `range`: its spans clipped to the range,
    /// its extent narrowed to the range, and the same label and transformation.
    ///
    /// Spans are half-open, so the range is read as the instants it contains:
    /// `a..b` is `[a, b)`, `a..=b` is `[a, b + 1)`, and an unbounded side
    /// reaches `i64::MIN` or `i64::MAX`, as [`Extent::everything`] does. The
    /// instant `i64::MAX` cannot be covered by a half-open span, so an
    /// inclusive end of `i64::MAX` reaches as far as an unbounded one. A
    /// reversed or empty range holds no instant: the result is empty and
    /// observed nowhere, rather than a panic.
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<Timestamp>) -> Self {
        let window = Extent::of_range(&range);
        let mut observed = self.observed.clone();
        observed.narrow(&window);

        Self::from_parts(
            self.attribute.clone(),
            self.transformation,
            intersection_spans(&self.spans, window.as_slice()),
            observed,
        )
    }

    /// Hand over the final part: the spans ending before `frontier`, whole,
    /// and the extent before the earliest instant this keeps.
    ///
    /// A span reaching `frontier` stays, since it may still grow. That instant
    /// is the start of the first span kept, or `frontier` when none is, so the
    /// two halves share the extent and the data of each lies inside its own.
    pub(crate) fn split_before(&mut self, frontier: Timestamp) -> Self {
        let split = self.spans.partition_point(|span| span.end() < frontier);
        let cut = self
            .spans
            .get(split)
            .map_or(frontier, |span| span.start().min(frontier));

        let kept = self.spans.split_off(split);
        let finalised = std::mem::replace(&mut self.spans, kept);

        let mut finalised_observed = self.observed.clone();
        finalised_observed.narrow(&Extent::of_range(&(..cut)));
        let mut kept_observed = self.observed.clone();
        kept_observed.narrow(&Extent::of_range(&(cut..)));

        let remaining = Self::from_parts(
            self.attribute.clone(),
            self.transformation,
            std::mem::take(&mut self.spans),
            kept_observed,
        );
        let drained = Self::from_parts(
            self.attribute.clone(),
            self.transformation,
            finalised,
            finalised_observed,
        );
        *self = remaining;
        drained
    }
}
