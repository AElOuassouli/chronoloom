//! `Edges`: the starts and ends of the spans of a timeline.

use crate::primitives::{TimePointEvent, Timestamp};
use crate::processing::{Edge, ProcessError};
use crate::sequences::attribute::Attribute;
use crate::sequences::{TimeIntervalSequence, TimePointSequence};
use crate::store::{AnyPointSequence, Sequence, Store};

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::online_points::OnlinePoints;
use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Edges {
    inner: Node,
    label: String,
}

impl Edges {
    pub(super) const fn new(inner: Node, label: String) -> Self {
        Self { inner, label }
    }

    pub(super) fn online(&self) -> OnlinePoints {
        OnlinePoints::Edges(OnlineEdges {
            inner: Box::new(self.inner.online()),
            label: self.label.clone(),
            open: None,
        })
    }

    pub(super) fn batch(&self, inputs: &Store) -> Result<AnyPointSequence, ProcessError> {
        let timeline = self.inner.batch(inputs)?;
        Ok(edges_of(&timeline, &self.label))
    }
}

/// The starts and ends of the spans of `timeline`, over its extent.
fn edges_of(timeline: &TimeIntervalSequence, label: &str) -> AnyPointSequence {
    let edges = timeline
        .iter()
        .flat_map(|span| {
            [
                TimePointEvent::new(span.start(), Edge::Start),
                TimePointEvent::new(span.end(), Edge::End),
            ]
        })
        .collect();

    AnyPointSequence::from(TimePointSequence::from_parts(
        Attribute::name(label.to_owned()),
        edges,
        timeline.observed().clone(),
    ))
}

/// The running state of [`Edges`].
///
/// The edges of a chunk are those of the timeline joined to the span still open
/// at the end of the chunk before, so a span cut at a boundary has neither an
/// end nor a start there. `open` is that span, as a chunk of the timeline from
/// its start to the frontier, or `None` when no span reaches the frontier.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct OnlineEdges {
    inner: Box<OnlineNode>,
    label: String,
    open: Option<Chunk>,
}

impl OnlineEdges {
    /// # Panics
    ///
    /// If the inner node gives a chunk that is not a timeline, which no online
    /// node does.
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let frontier = self.inner.frontier();
        let chunk = self.inner.step(inputs, to)?;
        let joined = match self.open.take() {
            Some(earlier) => earlier.join(chunk),
            None => chunk,
        };
        let timeline = joined.timeline().expect("online nodes give timelines");

        // An end at the end of the chunk belongs to the chunk after, and the
        // edges before the frontier were given with the chunks before.
        let edges = Sequence::TimePoint(edges_of(timeline, &self.label));
        // The one end that belongs to this chunk is the end of time, where a
        // span that never ends stops.
        let output = if joined.to() == Timestamp::MAX {
            Chunk::new(frontier, Timestamp::MAX, edges.slice(frontier..))
        } else {
            Chunk::of(&edges, frontier, joined.to())
        };

        self.open = timeline
            .as_slice()
            .last()
            .filter(|span| span.end() == joined.to())
            .map(|span| joined.slice(span.start(), joined.to()));
        Ok(output)
    }

    pub(super) fn frontier(&self) -> Timestamp {
        self.inner.frontier()
    }
}

#[cfg(feature = "serde")]
impl OnlineEdges {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.label == fresh.label && self.inner.same_plan(&fresh.inner)
    }
}
