//! `Occurs`: `[t, t + 1)` for each matching point inside the extent.

use crate::primitives::TimeIntervalEvent;
use crate::processing::read;
use crate::processing::{Pattern, ProcessError};
use crate::sequence_system::SequenceSystem;
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::TimeIntervalSequence;

#[derive(Debug, Clone)]
pub(super) struct Occurs {
    pattern: Pattern,
}

impl Occurs {
    pub(super) fn new(pattern: Pattern) -> Self {
        Self { pattern }
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
