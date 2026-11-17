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
}
