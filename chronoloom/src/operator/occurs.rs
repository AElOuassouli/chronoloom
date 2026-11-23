//! `Occurs`: `[t, t + 1)` for each matching point inside the extent.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{Pattern, ProcessError};
use crate::sequence_system::{Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::reading::{mismatch, read};

#[derive(Debug, Clone)]
pub(super) struct Occurs {
    pattern: Pattern,
}

impl Occurs {
    pub(super) fn new(pattern: Pattern) -> Self {
        Self { pattern }
    }

    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::Occurs(OnlineOccurs {
            pattern: self.pattern.clone(),
            frontier: Timestamp::MIN,
        })
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let marks = read(&self.pattern, inputs)?;
        let spans = marks
            .points
            .iter()
            // An instant the extent contains is below `i64::MAX`, so `t + 1` holds.
            .filter(|(timestamp, matched)| *matched && marks.observed.contains(*timestamp))
            .map(|(timestamp, _)| TimeIntervalEvent::raw(*timestamp, *timestamp + 1))
            .collect();

        Ok(TimeIntervalSequence::from_parts(
            Attribute::name(format!("occurs({})", self.pattern)),
            (0, 0),
            normalize(spans),
            marks.observed.clone(),
        ))
    }
}

/// The running state of [`Occurs`]: each chunk of points read on its own.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlineOccurs {
    pattern: Pattern,
    frontier: Timestamp,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineOccurs {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let key = &self.pattern.key;
        let chunk = inputs
            .get(key)
            .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?
            .slice(self.frontier, to);
        let Sequence::TimePoint(points) = chunk.sequence() else {
            return Err(ProcessError::NotPoints { key: key.clone() });
        };

        let marks = self
            .pattern
            .condition
            .marks(points)
            .ok_or_else(|| mismatch(&self.pattern, points))?;
        let observed = points.observed();
        let spans = marks
            .iter()
            // An instant the extent contains is below `i64::MAX`, so `t + 1` holds.
            .filter(|(timestamp, matched)| *matched && observed.contains(*timestamp))
            .map(|(timestamp, _)| TimeIntervalEvent::raw(*timestamp, *timestamp + 1))
            .collect();

        let timeline = TimeIntervalSequence::from_parts(
            Attribute::name(format!("occurs({})", self.pattern)),
            (0, 0),
            normalize(spans),
            observed.clone(),
        );
        self.frontier = chunk.to();
        Ok(Chunk::new(
            chunk.from(),
            chunk.to(),
            Sequence::TimeInterval(timeline),
        ))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}
