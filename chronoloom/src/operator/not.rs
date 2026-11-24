//! `Not`: the observed instants where the input is not active.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequences::TimeIntervalSequence;
use crate::store::{Sequence, Store};

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

    pub(super) fn batch(&self, inputs: &Store) -> Result<TimeIntervalSequence, ProcessError> {
        Ok(self.input.batch(inputs)?.complement())
    }
}

/// The running state of [`Not`]: the complement of each chunk within its
/// extent.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg(feature = "serde")]
impl OnlineNot {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.input.same_plan(&fresh.input)
    }
}
