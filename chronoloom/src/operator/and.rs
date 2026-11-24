//! `And`: active where every input is.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::combination::Combination;
use super::online::OnlineNode;
use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct And {
    parts: Vec<Node>,
}

impl And {
    pub(super) fn new(parts: Vec<Node>) -> Self {
        Self { parts }
    }

    /// The running state.
    pub(super) fn online(&self) -> OnlineNode {
        let parts = self.parts.iter().map(Node::online).collect();

        OnlineNode::And(OnlineAnd(Combination::new(
            parts,
            TimeIntervalSequence::from_spans(
                "⊤",
                vec![TimeIntervalEvent::raw(Timestamp::MIN, Timestamp::MAX)],
            ),
            TimeIntervalSequence::intersection,
        )))
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let mut parts = self.parts.iter();
        let Some(first) = parts.next() else {
            // The identity of the intersection: active everywhere.
            let everywhere = TimeIntervalEvent::raw(Timestamp::MIN, Timestamp::MAX);
            return Ok(TimeIntervalSequence::from_spans("⊤", vec![everywhere]));
        };

        let mut timeline = first.batch(inputs)?;
        for part in parts {
            timeline = timeline.intersection(&part.batch(inputs)?);
        }
        Ok(timeline)
    }
}

/// The running state of [`And`]: the intersection of its parts' chunks.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Combination"))]
pub(crate) struct OnlineAnd(Combination);

#[cfg(feature = "serde")]
impl From<Combination> for OnlineAnd {
    fn from(combination: Combination) -> Self {
        Self(combination.with(TimeIntervalSequence::intersection))
    }
}

impl OnlineAnd {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        self.0.step(inputs, to)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.0.frontier()
    }
}

#[cfg(feature = "serde")]
impl OnlineAnd {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.0.same_plan(&fresh.0)
    }
}
