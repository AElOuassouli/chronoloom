//! `Changes`: the points of a member with the repeats dropped.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::{AnyPointSequence, Key, Sequence, SequenceSystem};

use super::chunk::{Chunk, Inputs};
use super::online_points::OnlinePoints;
use super::reading::points;

#[derive(Debug, Clone)]
pub(super) struct Changes {
    key: Key,
    label: String,
}

impl Changes {
    pub(super) const fn new(key: Key, label: String) -> Self {
        Self { key, label }
    }

    pub(super) fn online(&self) -> OnlinePoints {
        OnlinePoints::Changes(OnlineChanges {
            key: self.key.clone(),
            label: self.label.clone(),
            frontier: Timestamp::MIN,
            last_sample: None,
        })
    }

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        let mut changes = points(&self.key, inputs)?.changes();
        changes.set_attribute(self.label.clone());

        Ok(changes)
    }
}

/// The running state of [`Changes`].
///
/// A point is kept if it differs from the sample before it within the same run
/// of the extent, so a chunk needs the last sample of the chunks before. What
/// is held is the input from the instant of that sample to the frontier: the
/// sample, and the extent that says whether the run it belongs to goes on into
/// the next chunk.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct OnlineChanges {
    key: Key,
    label: String,
    frontier: Timestamp,
    last_sample: Option<Chunk>,
}

impl OnlineChanges {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let chunk = inputs
            .get(&self.key)
            .ok_or_else(|| ProcessError::UnknownKey {
                key: self.key.clone(),
            })?
            .slice(self.frontier, to);
        let Sequence::TimePoint(_) = chunk.sequence() else {
            return Err(ProcessError::NotPoints {
                key: self.key.clone(),
            });
        };

        let frontier = self.frontier;
        let joined = match self.last_sample.take() {
            Some(earlier) => earlier.join(chunk),
            None => chunk,
        };
        let Sequence::TimePoint(points) = joined.sequence() else {
            unreachable!("chunks of one member are points")
        };

        let mut changes = points.changes();
        changes.set_attribute(self.label.clone());
        let output = Chunk::of(&Sequence::TimePoint(changes), frontier, joined.to());

        self.last_sample = points
            .instants_in(Timestamp::MIN, Timestamp::MAX)
            .last()
            .map(|instant| joined.slice(*instant, joined.to()));
        self.frontier = joined.to();
        Ok(output)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}

#[cfg(feature = "serde")]
impl OnlineChanges {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.key == fresh.key && self.label == fresh.label
    }
}
