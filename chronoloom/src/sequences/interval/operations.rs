//! The four set operations over timelines.
//!
//! All four share the same shape, so none of the methods below restates it:
//! both operands are borrowed and left untouched, the result is a new sequence,
//! and because both inputs are already ordered the work is a single pass over
//! the two — linear in their combined length, with no sorting. Each result
//! describes itself as the operation applied to what its operands describe, so
//! `up.union(&busy)` is `up ∪ busy`.

use crate::primitives::TimeIntervalEvent;
use crate::sequences::attribute::SetOperation;

use super::normalization::absorb;
use super::TimeIntervalSequence;

impl TimeIntervalSequence {
    /// The instants covered by **either** timeline.
    ///
    /// Spans that touch across the two merge, as they would within one:
    /// `[0, 5)` here and `[5, 9)` there become `[0, 9)`.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 5)?]);
    /// let extra = TimeIntervalSequence::from_spans("extra", vec![
    ///     TimeIntervalEvent::span(5, 9)?,
    ///     TimeIntervalEvent::span(20, 30)?,
    /// ]);
    ///
    /// let either = up.union(&extra);
    /// let bounds: Vec<(i64, i64)> = either.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 9), (20, 30)]);
    /// assert_eq!(either.to_string(), "up ∪ extra");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        // At most every span from both sides survives, so this capacity is an
        // exact upper bound.
        let mut spans = Vec::with_capacity(self.spans.len() + other.spans.len());
        let (mut i, mut j) = (0, 0);

        loop {
            // Take whichever side starts earlier. A tie can go either way,
            // since the two will merge regardless.
            let next = match (self.spans.get(i), other.spans.get(j)) {
                (Some(mine), Some(theirs)) => {
                    if mine.start() <= theirs.start() {
                        i += 1;
                        *mine
                    } else {
                        j += 1;
                        *theirs
                    }
                }
                (Some(mine), None) => {
                    i += 1;
                    *mine
                }
                (None, Some(theirs)) => {
                    j += 1;
                    *theirs
                }
                (None, None) => break,
            };

            absorb(&mut spans, next);
        }

        Self::from_parts(
            self.combined_attribute(SetOperation::Union, other),
            (0, 0),
            spans,
        )
    }

    /// The instants covered by **both** timelines.
    ///
    /// Because spans are half-open, timelines that merely touch share no
    /// instant and so intersect to nothing.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 100)?]);
    /// let busy = TimeIntervalSequence::from_spans("busy", vec![
    ///     TimeIntervalEvent::span(10, 20)?,
    ///     TimeIntervalEvent::span(30, 40)?,
    /// ]);
    ///
    /// // Everything busy happened while up, so the overlap covers `busy` exactly
    /// // — under a description of its own.
    /// let overlap = up.intersection(&busy);
    /// assert!(overlap.covers_same(&busy));
    /// assert_eq!(overlap.to_string(), "up ∩ busy");
    ///
    /// let touching = TimeIntervalSequence::from_spans("touching", vec![
    ///     TimeIntervalEvent::span(100, 200)?,
    /// ]);
    /// assert!(up.intersection(&touching).is_empty());
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        let mut spans = Vec::new();
        let (mut i, mut j) = (0, 0);

        while let (Some(mine), Some(theirs)) = (self.spans.get(i), other.spans.get(j)) {
            // What the pair shares is the primitive's business, not this loop's.
            if let Some(overlap) = mine.intersection(theirs) {
                spans.push(overlap);
            }

            // Whichever ends first cannot reach anything still to come, so it is
            // safe to drop. That is the entire trick.
            if mine.end() < theirs.end() {
                i += 1;
            } else {
                j += 1;
            }
        }

        Self::from_parts(
            self.combined_attribute(SetOperation::Intersection, other),
            (0, 0),
            spans,
        )
    }

    /// The instants covered by this timeline but **not** by `other`.
    ///
    /// A span of `other` landing inside one of ours splits it in two.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 100)?]);
    /// let maintenance = TimeIntervalSequence::from_spans("maintenance", vec![
    ///     TimeIntervalEvent::span(10, 20)?,
    ///     TimeIntervalEvent::span(30, 40)?,
    /// ]);
    ///
    /// let serving = up.difference(&maintenance);
    /// let bounds: Vec<(i64, i64)> = serving.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 10), (20, 30), (40, 100)]);
    /// assert_eq!(serving.to_string(), "up \\ maintenance");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn difference(&self, other: &Self) -> Self {
        let attribute = self.combined_attribute(SetOperation::Difference, other);

        // `cursor` is the first instant of the current span not yet accounted
        // for. Starting it needs a first span, so an empty timeline is done
        // before the loop begins.
        let Some(first) = self.spans.first() else {
            return Self::from_parts(attribute, (0, 0), Vec::new());
        };

        let mut spans = Vec::new();
        let (mut i, mut j) = (0, 0);
        let mut cursor = first.start();

        while let Some(mine) = self.spans.get(i) {
            match other.spans.get(j) {
                // Nothing of `other` reaches into what is left of this span.
                Some(theirs) if theirs.start() < mine.end() => {
                    if theirs.end() <= cursor {
                        // Already behind the cursor, so it covers nothing new.
                        j += 1;
                        continue;
                    }

                    if cursor < theirs.start() {
                        spans.push(TimeIntervalEvent::raw(cursor, theirs.start()));
                    }
                    // The guard above established `theirs.end() > cursor`, so
                    // this only ever moves the cursor forward.
                    cursor = theirs.end();

                    if cursor >= mine.end() {
                        // This span is fully accounted for, but `theirs` may
                        // still reach into the next one.
                        i += 1;
                    } else {
                        j += 1;
                    }
                }
                _ => {
                    if cursor < mine.end() {
                        spans.push(TimeIntervalEvent::raw(cursor, mine.end()));
                    }
                    i += 1;
                }
            }

            // Every new span starts uncovered.
            if let Some(next) = self.spans.get(i) {
                cursor = cursor.max(next.start());
            }
        }

        Self::from_parts(attribute, (0, 0), spans)
    }

    /// The instants covered by exactly **one** of the two timelines.
    ///
    /// The set-algebra exclusive-or: an instant belongs to the result when it
    /// is in this timeline or the other, but not both.
    ///
    /// Composed as `(self - other) ∪ (other - self)`, so still linear. That is
    /// how the spans are found, but not how the result describes itself: it is
    /// `self △ other`, the operation asked for, rather than the route taken to
    /// it.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let a = TimeIntervalSequence::from_spans("a", vec![TimeIntervalEvent::span(0, 10)?]);
    /// let b = TimeIntervalSequence::from_spans("b", vec![TimeIntervalEvent::span(5, 15)?]);
    ///
    /// let only_one = a.symmetric_difference(&b);
    /// let bounds: Vec<(i64, i64)> = only_one.iter().map(TimeIntervalEvent::bounds).collect();
    /// assert_eq!(bounds, [(0, 5), (10, 15)]);
    ///
    /// // Exactly the instants where the two disagree.
    /// assert!(only_one.contains(2));
    /// assert!(!only_one.contains(7));
    /// assert_eq!(only_one.to_string(), "a △ b");
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        let composed = self.difference(other).union(&other.difference(self));

        Self::from_parts(
            self.combined_attribute(SetOperation::SymmetricDifference, other),
            (0, 0),
            composed.spans,
        )
    }
}
