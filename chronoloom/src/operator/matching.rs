//! `Matching`: the points of a member whose value matches.

use crate::processing::{Pattern, ProcessError};
use crate::sequence_system::{AnyPointSequence, SequenceSystem};

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

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
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
