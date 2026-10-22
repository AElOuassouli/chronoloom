//! One member of a system: either shape of sequence.

use crate::sequences::attribute::Attribute;
use crate::sequences::{TimeIntervalSequence, TimePointSequence};

use super::erased::{AnyPointSequence, PointValue};

/// A sequence of either shape, as a [`SequenceSystem`] holds it.
///
/// Anything that converts into one can be inserted into a system directly, so
/// this is mostly met when reading a system back: matching on it says which
/// shape a member is.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
/// use chronoloom::sequence_system::Sequence;
///
/// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 10)?]);
///
/// match Sequence::from(up) {
///     Sequence::TimePoint(points) => unreachable!("{} holds points", points.label()),
///     Sequence::TimeInterval(timeline) => assert_eq!(timeline.label(), "up"),
/// }
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// [`SequenceSystem`]: super::SequenceSystem
#[derive(Debug, Clone, PartialEq)]
pub enum Sequence {
    /// Many observations over time, each carrying its own value.
    TimePoint(AnyPointSequence),
    /// One state over time, as the spans during which it was active.
    TimeInterval(TimeIntervalSequence),
}

impl Sequence {
    /// The attribute the sequence corresponds to, which a system files it
    /// under.
    ///
    /// For a timeline this is the whole description, its [`transformation`]
    /// included: `A` shifted by `[1, 2]` corresponds to `A[1, 2]`, not to `A`.
    ///
    /// [`transformation`]: TimeIntervalSequence::transformation
    #[must_use]
    pub fn attribute(&self) -> Attribute {
        match self {
            Self::TimePoint(points) => points.attribute().clone(),
            Self::TimeInterval(timeline) => timeline.folded_attribute(),
        }
    }

    /// The sequence's [`attribute`] written out — the text a system files it
    /// under.
    ///
    /// [`attribute`]: Sequence::attribute
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::TimePoint(points) => points.label(),
            Self::TimeInterval(timeline) => timeline.label(),
        }
    }

    /// How many events a point sequence holds, or how many spans a timeline
    /// holds after merging.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::TimePoint(points) => points.len(),
            Self::TimeInterval(timeline) => timeline.len(),
        }
    }

    /// Whether the sequence holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The point sequence, if this is one and its events carry values of type
    /// `T`.
    #[must_use]
    pub fn as_time_point<T: 'static>(&self) -> Option<&TimePointSequence<T>> {
        match self {
            Self::TimePoint(points) => points.downcast_ref(),
            Self::TimeInterval(_) => None,
        }
    }

    /// The timeline, if this is one.
    #[must_use]
    pub const fn as_time_interval(&self) -> Option<&TimeIntervalSequence> {
        match self {
            Self::TimePoint(_) => None,
            Self::TimeInterval(timeline) => Some(timeline),
        }
    }
}

impl<T: PointValue> From<TimePointSequence<T>> for Sequence {
    fn from(sequence: TimePointSequence<T>) -> Self {
        Self::TimePoint(sequence.into())
    }
}

impl From<AnyPointSequence> for Sequence {
    fn from(sequence: AnyPointSequence) -> Self {
        Self::TimePoint(sequence)
    }
}

impl From<TimeIntervalSequence> for Sequence {
    fn from(sequence: TimeIntervalSequence) -> Self {
        Self::TimeInterval(sequence)
    }
}
