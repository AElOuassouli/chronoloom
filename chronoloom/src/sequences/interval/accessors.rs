//! Reading a timeline: what it describes, how much it covers, and what sits at
//! a given instant.

use std::slice;

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::sequences::attribute::{Attribute, SetOperation};

use super::TimeIntervalSequence;

impl TimeIntervalSequence {
    /// What this timeline is the timeline of.
    ///
    /// See [the type documentation][what] for how a derived timeline comes by
    /// its description.
    ///
    /// [what]: TimeIntervalSequence#what-the-timeline-describes
    #[must_use]
    pub const fn attribute(&self) -> &Attribute {
        &self.attribute
    }

    /// Describe this timeline as a state named outright, forgetting how it was
    /// derived.
    ///
    /// Naming a derived timeline replaces the whole expression — including the
    /// [`transformation`], which the old description was measured against and
    /// the new one is not.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 100)?]);
    /// let busy = TimeIntervalSequence::from_spans("busy", vec![TimeIntervalEvent::span(10, 20)?]);
    ///
    /// let mut idle = up.difference(&busy);
    /// assert_eq!(idle.to_string(), "up \\ busy");
    ///
    /// idle.set_attribute("idle");
    /// assert_eq!(idle.to_string(), "idle");
    /// assert_eq!(idle.transformation(), (0, 0));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [`transformation`]: TimeIntervalSequence::transformation
    pub fn set_attribute(&mut self, attribute: impl Into<String>) {
        self.attribute = Attribute::name(attribute);
        self.transformation = (0, 0);
    }

    /// The `[alpha, beta]` shift [`transform`] has applied so far, `(0, 0)`
    /// until one does.
    ///
    /// [`transform`]: TimeIntervalSequence::transform
    #[must_use]
    pub const fn transformation(&self) -> (Timestamp, Timestamp) {
        self.transformation
    }

    /// How this timeline describes itself, as an owned `String` — the same text
    /// [`Display`] writes.
    ///
    /// [`Display`]: core::fmt::Display
    #[must_use]
    pub fn label(&self) -> String {
        self.to_string()
    }

    /// Whether two timelines cover exactly the same instants, whatever they
    /// describe.
    ///
    /// Equality asks more than this — it asks that the two describe the same
    /// thing as well — so this is what the set-algebra identities are stated
    /// with: `A ∪ B` and `B ∪ A` cover the same instants under different
    /// descriptions.
    #[must_use]
    pub fn covers_same(&self, other: &Self) -> bool {
        self.spans == other.spans
    }

    /// How this timeline enters an operation: its attribute, carrying whatever
    /// it has been transformed by.
    ///
    /// The result of an operation has no transformation of its own, so an
    /// operand's has to be written into the description at the moment it is
    /// combined — which is exactly what makes `A[1, 2] ∪ B` say something
    /// `A ∪ B` would not.
    pub(super) fn folded_attribute(&self) -> Attribute {
        let (alpha, beta) = self.transformation;

        self.attribute.clone().transformed(alpha, beta)
    }

    /// The description of `self` and `other` put through `operation`.
    pub(super) fn combined_attribute(&self, operation: SetOperation, other: &Self) -> Attribute {
        Attribute::combined(operation, self.folded_attribute(), other.folded_attribute())
    }

    /// Consume the sequence and return its spans, earliest first.
    ///
    /// The inverse of [`from_spans`], and free: the sequence hands over the
    /// `Vec` it was already holding. These are the normalized spans, not
    /// whatever was originally inserted.
    ///
    /// [`from_spans`]: TimeIntervalSequence::from_spans
    #[must_use]
    pub fn into_spans(self) -> Vec<TimeIntervalEvent<()>> {
        self.spans
    }

    /// How many spans the sequence holds, counting those that remain **after**
    /// merging.
    // Not a `const fn`: `Vec::len` is const-stable only from 1.87, past this
    // crate's 1.83 minimum supported Rust version.
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// How much time the sequence covers in total, in the same ticks as its
    /// bounds.
    ///
    /// A constant-time read of a maintained total, not a walk. It counts
    /// *covered instants*, not inserted spans — see [the type
    /// documentation][duration] for why that distinction matters. Unsigned
    /// because a total is a magnitude, and wide enough for every sequence
    /// that can be built: disjoint spans inside `i64` cannot total more than
    /// `i64::MAX - i64::MIN`, which is exactly [`u64::MAX`].
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let mut uptime = TimeIntervalSequence::new("uptime");
    /// uptime.insert(TimeIntervalEvent::span(0, 10)?);
    /// assert_eq!(uptime.active_duration(), 10);
    ///
    /// // Overlapping: only the five new ticks count.
    /// uptime.insert(TimeIntervalEvent::span(5, 15)?);
    /// assert_eq!(uptime.active_duration(), 15);
    ///
    /// // Already covered: nothing to add.
    /// uptime.insert(TimeIntervalEvent::span(2, 8)?);
    /// assert_eq!(uptime.active_duration(), 15);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    ///
    /// [duration]: TimeIntervalSequence#active-duration
    #[must_use]
    pub const fn active_duration(&self) -> u64 {
        self.active_duration
    }

    /// Whether the sequence covers no instants at all.
    // Not a `const fn`, for the same reason as `len`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The whole sequence as a slice, earliest span first. Constant time — the
    /// spans already sit contiguously.
    #[must_use]
    pub fn as_slice(&self) -> &[TimeIntervalEvent<()>] {
        &self.spans
    }

    /// The span at position `index`, counting from the earliest.
    #[must_use]
    pub fn nth(&self, index: usize) -> Option<&TimeIntervalEvent<()>> {
        self.spans.get(index)
    }

    /// Walk every span, earliest first.
    pub fn iter(&self) -> slice::Iter<'_, TimeIntervalEvent<()>> {
        self.spans.iter()
    }

    /// The earliest span, or `None` when the sequence covers nothing.
    #[must_use]
    pub fn first(&self) -> Option<&TimeIntervalEvent<()>> {
        self.spans.first()
    }

    /// The latest span, or `None` when the sequence covers nothing.
    #[must_use]
    pub fn last(&self) -> Option<&TimeIntervalEvent<()>> {
        self.spans.last()
    }

    /// The span covering `timestamp`, or `None` when the state was inactive.
    ///
    /// At most one span can cover an instant, because the sequence is disjoint.
    /// Spans are half-open, so a span's `start` is covered and its `end` is
    /// not. Logarithmic.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::TimeIntervalSequence;
    ///
    /// let uptime = TimeIntervalSequence::from_spans("uptime", vec![
    ///     TimeIntervalEvent::span(10, 20)?,
    /// ]);
    ///
    /// assert_eq!(uptime.at(15).map(TimeIntervalEvent::bounds), Some((10, 20)));
    /// assert_eq!(uptime.at(10).map(TimeIntervalEvent::bounds), Some((10, 20)));
    /// assert_eq!(uptime.at(20), None);
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    #[must_use]
    pub fn at(&self, timestamp: Timestamp) -> Option<&TimeIntervalEvent<()>> {
        // The only candidate is the last span starting at or before the
        // instant; anything earlier ended before it, anything later starts
        // after it.
        let candidate = self
            .spans
            .partition_point(|span| span.start() <= timestamp)
            .checked_sub(1)?;

        self.spans
            .get(candidate)
            .filter(|span| span.end() > timestamp)
    }

    /// Whether the state was active at `timestamp`.
    ///
    /// Half-open, as everywhere: a span covers its `start` but not its `end`.
    /// [`at`] returns the span itself and shows that edge.
    ///
    /// [`at`]: TimeIntervalSequence::at
    #[must_use]
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        self.at(timestamp).is_some()
    }
}
