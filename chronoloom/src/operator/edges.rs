//! `Edges`: the starts and ends of the spans of a timeline.

use crate::primitives::TimePointEvent;
use crate::processing::{Edge, ProcessError};
use crate::sequence_system::{AnyPointSequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::TimePointSequence;

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

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        let timeline = self.inner.batch(inputs)?;
        let edges = timeline
            .iter()
            .flat_map(|span| {
                [
                    TimePointEvent::new(span.start(), Edge::Start),
                    TimePointEvent::new(span.end(), Edge::End),
                ]
            })
            .collect();

        Ok(AnyPointSequence::from(TimePointSequence::from_parts(
            Attribute::name(self.label.clone()),
            edges,
            timeline.observed().clone(),
        )))
    }
}
