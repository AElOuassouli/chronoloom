//! `And`: active where every input is.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct And {
    parts: Vec<Node>,
}

impl And {
    pub(super) fn new(parts: Vec<Node>) -> Self {
        Self { parts }
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
