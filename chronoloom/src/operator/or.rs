//! `Or`: active where at least one input is.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::combination::Combination;
use super::online::OnlineNode;
use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Or {
    parts: Vec<Node>,
}

impl Or {
    pub(super) fn new(parts: Vec<Node>) -> Self {
        Self { parts }
    }

    /// The running state.
    pub(super) fn online(&self) -> OnlineNode {
        let parts = self.parts.iter().map(Node::online).collect();

        OnlineNode::Or(OnlineOr(Combination::new(
            parts,
            TimeIntervalSequence::new("⊥"),
            TimeIntervalSequence::union,
        )))
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let mut parts = self.parts.iter();
        let Some(first) = parts.next() else {
            // The identity of the union: active nowhere.
            return Ok(TimeIntervalSequence::new("⊥"));
        };

        let mut timeline = first.batch(inputs)?;
        for part in parts {
            timeline = timeline.union(&part.batch(inputs)?);
        }
        Ok(timeline)
    }
}

/// The running state of [`Or`]: the union of its parts' chunks.
#[derive(Debug, Clone)]
pub(crate) struct OnlineOr(Combination);

impl OnlineOr {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        self.0.step(inputs, to)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.0.frontier()
    }
}
