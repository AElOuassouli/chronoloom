//! `While`: each sample holds until the next one, or until its timeout, within
//! a run of the extent.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{Pattern, ProcessError};
use crate::sequence_system::SequenceSystem;
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence};

use super::reading::read;

#[derive(Debug, Clone)]
pub(super) struct While {
    pattern: Pattern,
    timeout: Option<Timestamp>,
}

impl While {
    pub(super) const fn new(pattern: Pattern, timeout: Option<Timestamp>) -> Self {
        Self { pattern, timeout }
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let marks = read(&self.pattern, inputs)?;
        let mut spans = Vec::new();
        let mut known = Vec::new();
        let mut cursor = 0;

        for run in marks.observed.as_slice() {
            let (run_start, run_end) = run.bounds();

            while cursor < marks.points.len() && marks.points[cursor].0 < run_start {
                cursor += 1;
            }
            let first = cursor;
            while cursor < marks.points.len() && marks.points[cursor].0 < run_end {
                cursor += 1;
            }
            let samples = &marks.points[first..cursor];

            // Before the first sample of the run nothing is known.
            let mut index = 0;
            while index < samples.len() {
                // Of the points sharing an instant, the last recorded is the sample.
                let (timestamp, _) = samples[index];
                let mut last = index;
                while samples
                    .get(last + 1)
                    .is_some_and(|next| next.0 == timestamp)
                {
                    last += 1;
                }

                // The sample holds until the next one, or until its timeout, after
                // which nothing is known until the next sample.
                let next = samples.get(last + 1).map_or(run_end, |next| next.0);
                let until = self
                    .timeout
                    .map_or(next, |timeout| next.min(timestamp.saturating_add(timeout)));
                known.push(TimeIntervalEvent::raw(timestamp, until));
                if samples[last].1 {
                    spans.push(TimeIntervalEvent::raw(timestamp, until));
                }
                index = last + 1;
            }
        }

        Ok(TimeIntervalSequence::from_parts(
            Attribute::name(format!("while({})", self.pattern)),
            (0, 0),
            normalize(spans),
            Extent::from_normalized(normalize(known)),
        ))
    }
}
