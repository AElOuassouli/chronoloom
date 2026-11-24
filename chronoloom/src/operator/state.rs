//! `State`: a timeline member, as it is.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequences::TimeIntervalSequence;
use crate::store::{Key, Sequence, Store};

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;

#[derive(Debug, Clone)]
pub(super) struct State {
    key: Key,
}

impl State {
    pub(super) fn new(key: Key) -> Self {
        Self { key }
    }

    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::State(OnlineState {
            key: self.key.clone(),
            frontier: Timestamp::MIN,
        })
    }

    pub(super) fn batch(&self, inputs: &Store) -> Result<TimeIntervalSequence, ProcessError> {
        match inputs.get(self.key.as_str()) {
            Some(Sequence::TimeInterval(timeline)) => Ok(timeline.clone()),
            Some(Sequence::TimePoint(_)) => Err(ProcessError::NotATimeline {
                key: self.key.clone(),
            }),
            None => Err(ProcessError::UnknownKey {
                key: self.key.clone(),
            }),
        }
    }
}

/// The running state of [`State`]: the member's chunks, as they come.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct OnlineState {
    key: Key,
    frontier: Timestamp,
}

impl OnlineState {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let chunk = inputs
            .get(&self.key)
            .ok_or_else(|| ProcessError::UnknownKey {
                key: self.key.clone(),
            })?;
        if chunk.timeline().is_none() {
            return Err(ProcessError::NotATimeline {
                key: self.key.clone(),
            });
        }

        let output = chunk.slice(self.frontier, to);
        self.frontier = output.to();
        Ok(output)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}

#[cfg(feature = "serde")]
impl OnlineState {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.key == fresh.key
    }
}
