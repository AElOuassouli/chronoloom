//! `Not`: the observed instants where the input is not active.

use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

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

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        Ok(self.input.batch(inputs)?.complement())
    }
}
