//! `Matching`: the points of a member whose value matches.

use crate::primitives::Timestamp;
use crate::processing::{Pattern, ProcessError};
use crate::store::{AnyPointSequence, Sequence, Store};

use super::chunk::{Chunk, Inputs};
use super::online_points::OnlinePoints;
use super::reading::{mismatch, points};

#[derive(Debug, Clone)]
pub(super) struct Matching {
    pattern: Pattern,
    label: String,
}

impl Matching {
    pub(super) const fn new(pattern: Pattern, label: String) -> Self {
        Self { pattern, label }
    }

    pub(super) fn online(&self) -> OnlinePoints {
        OnlinePoints::Matching(OnlineMatching {
            pattern: self.pattern.clone(),
            label: self.label.clone(),
            frontier: Timestamp::MIN,
        })
    }

    pub(super) fn batch(&self, inputs: &Store) -> Result<AnyPointSequence, ProcessError> {
        let points = points(&self.pattern.key, inputs)?;
        let mut matching = self
            .pattern
            .condition
            .matching(points)
            .ok_or_else(|| mismatch(&self.pattern, points))?;
        matching.set_attribute(self.label.clone());

        Ok(matching)
    }
}

/// The running state of [`Matching`]: each chunk of points read on its own.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct OnlineMatching {
    pattern: Pattern,
    label: String,
    frontier: Timestamp,
}

impl OnlineMatching {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let key = &self.pattern.key;
        let chunk = inputs
            .get(key)
            .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?
            .slice(self.frontier, to);
        let Sequence::TimePoint(points) = chunk.sequence() else {
            return Err(ProcessError::NotPoints { key: key.clone() });
        };

        let mut matching = self
            .pattern
            .condition
            .matching(points)
            .ok_or_else(|| mismatch(&self.pattern, points))?;
        matching.set_attribute(self.label.clone());

        self.frontier = chunk.to();
        Ok(Chunk::new(
            chunk.from(),
            chunk.to(),
            Sequence::TimePoint(matching),
        ))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}

#[cfg(feature = "serde")]
impl OnlineMatching {
    /// Whether the fixed parts equal those of `fresh`, compiled from the processor.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.pattern == fresh.pattern && self.label == fresh.label
    }
}
