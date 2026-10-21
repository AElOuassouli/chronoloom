//! A normalized timeline of the spans during which one state was active.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::sequences::attribute::Attribute;

use self::normalization::{normalize, span_ticks, OVERFLOW};

mod accessors;
mod mutation;
mod normalization;
mod operations;
mod traits;
mod transform;

/// The spans during which one state was active, in canonical form.
///
/// Every span in a sequence means the same thing, so the sequence describes a
/// single state over time rather than a collection of unrelated intervals. It
/// is kept **normalized**: sorted by start, pairwise disjoint, and with no two
/// spans left touching. Overlapping and touching spans are merged as they
/// arrive, since `[0, 5)` and `[5, 9)` together cover exactly `[0, 9)` — the
/// same rule [`TimeIntervalEvent::union`] applies to a pair.
///
/// # Coverage, not history
///
/// Normalization means a sequence records *which instants are covered*, not
/// which spans were inserted. Two timelines of the same state are equal exactly
/// when they cover the same instants, however they were built, and [`len`]
/// counts the spans that remain after merging rather than the number inserted.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let piecemeal = TimeIntervalSequence::from_spans("up", vec![
///     TimeIntervalEvent::span(0, 5)?,
///     TimeIntervalEvent::span(5, 10)?,
///     TimeIntervalEvent::span(2, 7)?,
/// ]);
/// let whole = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 10)?]);
///
/// // Three spans in, one span out — and the two timelines are the same.
/// assert_eq!(piecemeal.len(), 1);
/// assert_eq!(piecemeal, whole);
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// # What the timeline describes
///
/// No span carries a value, because every span means the same thing: the value
/// belongs to the timeline as a whole. That is the [`attribute`] — the state
/// this is the timeline *of* — and every sequence has one. There is no such
/// thing as an unnamed timeline, which is why the constructors ask for a name
/// before they ask for spans.
///
/// Alongside it sits the [`transformation`], the `[alpha, beta]` shift
/// [`transform`] has applied so far. The two render together as the label
/// `A[alpha, beta]`, and `[0, 0]` — the untransformed case — is left off. An
/// operation describes its result out of the two labels that went into it, so a
/// derived timeline still says what it is with no name to invent; its own
/// transformation starts over at `[0, 0]`, since whatever the operands were
/// shifted by is already spelled out inside the label.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let a = TimeIntervalSequence::from_spans("A", vec![TimeIntervalEvent::span(0, 10)?]);
/// let b = TimeIntervalSequence::from_spans("B", vec![TimeIntervalEvent::span(20, 30)?]);
///
/// let either = a.transform(1, 2)?.union(&b);
/// assert_eq!(either.to_string(), "A[1, 2] ∪ B");
/// assert_eq!(either.transformation(), (0, 0));
///
/// // And the description keeps composing, grouped where it has to be.
/// assert_eq!(either.intersection(&b).to_string(), "(A[1, 2] ∪ B) ∩ B");
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// Because the label is part of what a sequence *is*, it is part of equality
/// too: two timelines are equal when they describe the same thing *and* cover
/// the same instants. `up` and `reachable` are two states even on a day they
/// coincided, so they are never equal — [`covers_same`] is the question that
/// asks only the second half.
///
/// # Active duration
///
/// The sequence carries the total time it covers as it goes, so
/// [`active_duration`] is a read rather than a walk. Normalization is what
/// makes that total meaningful: because the spans are disjoint, it is their
/// plain sum, and an instant covered by three overlapping inserts still counts
/// once.
///
/// # Transforming a timeline
///
/// [`transform`] moves every span's bounds — `alpha` the lower, `beta` the
/// upper — turning `[s, e)` into `[s + alpha, e + beta)`. Only the difference
/// `delta = alpha - beta` decides what happens structurally: a span survives
/// exactly when it is wider than `delta`, and a gap of `g` closes exactly when
/// `g + delta <= 0`. So one call either narrows or widens, never both. A call
/// that can drop spans (`delta > 0`) also widens every gap and can merge
/// nothing; a call that can merge (`delta < 0`) also widens every span and can
/// drop nothing.
///
/// The spans it moves are the normalized ones, so `[0, 5)` and `[5, 10)` are
/// one `[0, 10)` span by the time it sees them, and a narrowing takes one
/// span's worth of time from them rather than two. For the same reason
/// transforms compose — `A[a1, b1][a2, b2]` is `A[a1 + a2, b1 + b2]` — only
/// while nothing merges or vanishes: both discard structure no later transform
/// can recover.
///
/// The attribute is untouched and the shifts add to the [`transformation`], so
/// a second call reads as one transform rather than a stack of them, exactly as
/// that composition is written:
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let a = TimeIntervalSequence::from_spans("A", vec![TimeIntervalEvent::span(0, 100)?]);
///
/// assert_eq!(a.transform(1, 2)?.transform(3, 4)?.to_string(), "A[4, 6]");
///
/// // And a transform that undoes itself leaves nothing to say.
/// assert_eq!(a.transform(5, 5)?.transform(-5, -5)?.to_string(), "A");
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// The label is a description, not a recipe. Where that composition breaks
/// down — spans merged, spans vanished — the recorded shifts still total what
/// was asked for, which is what a reader of the timeline wants to know; they
/// will not rebuild it from the original.
///
/// [`active_duration`]: TimeIntervalSequence::active_duration
/// [`attribute`]: TimeIntervalSequence::attribute
/// [`covers_same`]: TimeIntervalSequence::covers_same
/// [`len`]: TimeIntervalSequence::len
/// [`transform`]: TimeIntervalSequence::transform
/// [`transformation`]: TimeIntervalSequence::transformation
#[derive(Debug, Clone)]
pub struct TimeIntervalSequence {
    /// Normalized: sorted by start, pairwise disjoint, and never touching — so
    /// `spans[i].end() < spans[i + 1].start()` strictly, for every adjacent
    /// pair. Every method restores this before returning.
    spans: Vec<TimeIntervalEvent<()>>,
    /// Total ticks covered by `spans`. Disjointness makes this their plain sum,
    /// with no instant counted twice; every method that touches `spans` updates
    /// it in the same breath.
    active_duration: u64,
    /// The state this is the timeline of. Never absent: a timeline that did not
    /// say what it describes would be a list of spans, not a state over time.
    attribute: Attribute,
    /// The `[alpha, beta]` shift [`TimeIntervalSequence::transform`] has applied
    /// so far, `(0, 0)` until one does. Kept beside the attribute rather than
    /// folded into it so the shift stays a number the caller can read back.
    transformation: (Timestamp, Timestamp),
}

impl TimeIntervalSequence {
    /// Create an empty timeline of the state named `attribute`.
    #[must_use]
    pub fn new(attribute: impl Into<String>) -> Self {
        Self {
            spans: Vec::new(),
            active_duration: 0,
            attribute: Attribute::name(attribute),
            transformation: (0, 0),
        }
    }

    /// Build a timeline of `attribute` from spans already in hand.
    ///
    /// The usual way to create one when the data exists up front. The spans may
    /// be in any order and may overlap freely; they are sorted once, in place,
    /// then merged into canonical form in a single pass.
    #[must_use]
    pub fn from_spans(attribute: impl Into<String>, spans: Vec<TimeIntervalEvent<()>>) -> Self {
        Self::from_parts(Attribute::name(attribute), (0, 0), normalize(spans))
    }

    /// Wrap spans that are already in canonical form, under a description.
    ///
    /// Private and unchecked: the caller must have produced them sorted,
    /// disjoint, and non-touching. Every use here either folds through
    /// [`absorb`] or walks two already-normalized sequences in order, both of
    /// which produce canonical output — so re-sorting would be wasted work.
    ///
    /// The one pass this does make is totalling the duration. Every caller is
    /// already linear in the spans, so it costs nothing asymptotically, and
    /// funnelling them all through here means no construction path can forget
    /// the total, or forget to say what the timeline describes.
    fn from_parts(
        attribute: Attribute,
        transformation: (Timestamp, Timestamp),
        spans: Vec<TimeIntervalEvent<()>>,
    ) -> Self {
        let active_duration = spans.iter().fold(0u64, |total, span| {
            total.checked_add(span_ticks(span)).expect(OVERFLOW)
        });

        Self {
            spans,
            active_duration,
            attribute,
            transformation,
        }
    }
}
