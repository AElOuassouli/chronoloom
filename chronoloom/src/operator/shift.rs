//! `Shift`: the timeline of the inner node, moved and widened.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::SequenceSystem;
use crate::sequences::TimeIntervalSequence;

use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Shift {
    inner: Box<Node>,
    alpha: Timestamp,
    beta: Timestamp,
}

impl Shift {
    pub(super) fn new(inner: Node, alpha: Timestamp, beta: Timestamp) -> Self {
        Self {
            inner: Box::new(inner),
            alpha,
            beta,
        }
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        Ok(self
            .inner
            .batch(inputs)?
            .transform_unbounded(self.alpha, self.beta))
    }
}
