//! What an online operator reads and writes: a sequence over a range of time.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::primitives::{TimePointEvent, Timestamp};
use crate::processing::Edge;
use crate::sequences::interval::operations::union_spans;
use crate::sequences::{Extent, TimeIntervalSequence};
use crate::store::{Key, Sequence};

/// A timeline or points over the range `[from, to)`, with its extent over
/// that range: the part of an output that became final in one step of a live
/// [`StreamSystem`].
///
/// Everything the sequence holds lies inside the range, and so does its
/// extent: an instant of the range outside the extent is unknown, as in a
/// whole sequence. Chunks of one output follow each other, and
/// [`join`](Chunk::join) of consecutive chunks is the sequence over the
/// whole range. A span cut at a chunk boundary merges back on `join`, as
/// touching spans do in any timeline, so no edge is left at the boundary.
///
/// One exception: a chunk that ends at the end of time may hold points at
/// `Timestamp::MAX`, such as the end of a span that never ends.
///
/// [`StreamSystem`]: crate::stream_system::StreamSystem
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    from: Timestamp,
    to: Timestamp,
    sequence: Sequence,
}

impl Chunk {
    /// The range of time the chunk covers: from its first instant to the first
    /// instant past it. Empty when the output made no progress.
    #[must_use]
    pub const fn range(&self) -> Range<Timestamp> {
        self.from..self.to
    }

    /// The timeline or points that became final over the range.
    #[must_use]
    pub const fn sequence(&self) -> &Sequence {
        &self.sequence
    }

    /// The starts and ends of the spans of a timeline chunk, as
    /// [`Events::Edges`](crate::Events::Edges) reads them, in time order;
    /// `None` for a chunk of points.
    ///
    /// The edges are those of the chunk on its own: a span the range cut has
    /// an edge at the cut, which the next chunk continues. Chunks of one
    /// output joined end to end have only the edges of the whole timeline.
    #[must_use]
    pub fn edges(&self) -> Option<Vec<TimePointEvent<Edge>>> {
        let timeline = self.timeline()?;
        Some(
            timeline
                .iter()
                .flat_map(|span| {
                    [
                        TimePointEvent::new(span.start(), Edge::Start),
                        TimePointEvent::new(span.end(), Edge::End),
                    ]
                })
                .collect(),
        )
    }

    /// The sequence over the range, taken out of the chunk.
    pub(crate) fn into_sequence(self) -> Sequence {
        self.sequence
    }

    /// A chunk over `[from, to)`; the sequence must lie inside the range.
    pub(crate) const fn new(from: Timestamp, to: Timestamp, sequence: Sequence) -> Self {
        Self { from, to, sequence }
    }

    /// The part of `sequence` inside `[from, to)`.
    pub(crate) fn of(sequence: &Sequence, from: Timestamp, to: Timestamp) -> Self {
        let to = to.max(from);
        Self::new(from, to, sequence.slice(from..to))
    }

    /// The first instant of the range.
    pub(crate) const fn from(&self) -> Timestamp {
        self.from
    }

    /// The first instant past the range.
    pub(crate) const fn to(&self) -> Timestamp {
        self.to
    }

    /// The timeline over the range, if the chunk holds one.
    pub(crate) fn timeline(&self) -> Option<&TimeIntervalSequence> {
        self.sequence.as_time_interval()
    }

    /// The part of the chunk inside `[from, to)`, cut to the range the chunk
    /// covers.
    pub(crate) fn slice(&self, from: Timestamp, to: Timestamp) -> Self {
        let from = from.max(self.from);
        let to = to.min(self.to).max(from);
        Self::of(&self.sequence, from, to)
    }

    /// This chunk followed by `next`, as one chunk over both ranges.
    ///
    /// # Panics
    ///
    /// If `next` does not start where this one ends, or the two hold different
    /// shapes or value types: operators produce consecutive chunks of one
    /// shape, so either is a bug.
    pub(crate) fn join(self, next: Self) -> Self {
        assert_eq!(self.to, next.from, "chunks join end to end");

        let sequence = match (self.sequence, next.sequence) {
            (Sequence::TimeInterval(first), Sequence::TimeInterval(second)) => {
                let spans = union_spans(first.as_slice(), second.as_slice());
                let observed =
                    union_spans(first.observed().as_slice(), second.observed().as_slice());
                Sequence::TimeInterval(TimeIntervalSequence::from_parts(
                    first.attribute().clone(),
                    first.transformation(),
                    spans,
                    Extent::from_normalized(observed),
                ))
            }
            (Sequence::TimePoint(mut first), Sequence::TimePoint(second)) => {
                assert!(
                    first.append(&second),
                    "chunks of one output carry one value type"
                );
                Sequence::TimePoint(first)
            }
            _ => panic!("chunks of one output have one shape"),
        };

        Self::new(self.from, next.to, sequence)
    }
}

/// The new chunk of each member an operator reads, by key.
#[derive(Debug, Clone, Default)]
pub(crate) struct Inputs {
    chunks: BTreeMap<Key, Chunk>,
}

impl Inputs {
    /// No input yet.
    pub(crate) const fn new() -> Self {
        Self {
            chunks: BTreeMap::new(),
        }
    }

    /// Give `chunk` as the new chunk of the member under `key`.
    pub(crate) fn insert(&mut self, key: Key, chunk: Chunk) {
        self.chunks.insert(key, chunk);
    }

    /// The new chunk of the member under `key`.
    pub(crate) fn get(&self, key: &Key) -> Option<&Chunk> {
        self.chunks.get(key)
    }
}
