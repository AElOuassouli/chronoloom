//! `State`: a timeline member, as it is.

use crate::processing::ProcessError;
use crate::sequence_system::{Key, Sequence, SequenceSystem};
use crate::sequences::TimeIntervalSequence;

#[derive(Debug, Clone)]
pub(super) struct State {
    key: Key,
}

impl State {
    pub(super) fn new(key: Key) -> Self {
        Self { key }
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
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
