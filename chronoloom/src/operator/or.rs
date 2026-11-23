//! `Or`: active where at least one input is.

use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Or {
    parts: Vec<Node>,
}

impl Or {
    pub(super) fn new(parts: Vec<Node>) -> Self {
        Self { parts }
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
