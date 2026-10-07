//! Changing a timeline, each method restoring the normalized invariant
//! and the covered total before it returns.

use crate::primitives::TimeIntervalEvent;

use super::normalization::{span_ticks, OVERFLOW, UNDERFLOW};
use super::TimeIntervalSequence;

impl TimeIntervalSequence {
    /// Drop every span, leaving the sequence covering nothing.
    pub fn clear(&mut self) {
        self.spans.clear();
        self.active_duration = 0;
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
    /// let mut uptime = TimeIntervalSequence::new("uptime");
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
    /// [`active_duration`]: TimeIntervalSequence::active_duration
    pub fn remove(&mut self, index: usize) -> TimeIntervalEvent<()> {
        let removed = self.spans.remove(index);
        self.active_duration = self
            .active_duration
            .checked_sub(span_ticks(&removed))
            .expect(UNDERFLOW);

        removed
    }
}
