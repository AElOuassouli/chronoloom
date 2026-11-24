//! `Not`: the observed instants where the input is not active.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::{Sequence, SequenceSystem};
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Not {
    input: Box<Node>,
}

impl Not {
    pub(super) fn new(input: Node) -> Self {
        Self {
            input: Box::new(input),
        }
    }

    /// The running state.
    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::Not(OnlineNot {
            input: Box::new(self.input.online()),
        })
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        Ok(self.input.batch(inputs)?.complement())
    }
}

/// The running state of [`Not`]: the complement of each chunk within its
/// extent.
#[derive(Debug, Clone)]
pub(crate) struct OnlineNot {
    input: Box<OnlineNode>,
}

impl OnlineNot {
    /// # Panics
    ///
    /// If the input gives a chunk that is not a timeline, which no online
    /// node does.
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let chunk = self.input.step(inputs, to)?;
        let timeline = chunk
            .timeline()
            .expect("online nodes give timelines")
            .complement();

        Ok(Chunk::new(
            chunk.from(),
            chunk.to(),
            Sequence::TimeInterval(timeline),
        ))
    }

    pub(super) fn frontier(&self) -> Timestamp {
        self.input.frontier()
    }
}
