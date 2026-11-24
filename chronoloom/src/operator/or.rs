//! `Or`: active where at least one input is.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequences::TimeIntervalSequence;
use crate::store::Store;

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

    pub(super) fn batch(&self, inputs: &Store) -> Result<TimeIntervalSequence, ProcessError> {
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Combination"))]
pub(crate) struct OnlineOr(Combination);

#[cfg(feature = "serde")]
impl From<Combination> for OnlineOr {
    fn from(combination: Combination) -> Self {
        Self(combination.with(TimeIntervalSequence::union))
    }
}

impl OnlineOr {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        self.0.step(inputs, to)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.0.frontier()
    }
}

#[cfg(feature = "serde")]
impl OnlineOr {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.0.same_plan(&fresh.0)
    }
}
