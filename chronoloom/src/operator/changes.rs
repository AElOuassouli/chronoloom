//! `Changes`: the points of a member with the repeats dropped.

use crate::processing::ProcessError;
use crate::sequence_system::{AnyPointSequence, Key, SequenceSystem};

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

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        let mut changes = points(&self.key, inputs)?.changes();
        changes.set_attribute(self.label.clone());

        Ok(changes)
    }
}
