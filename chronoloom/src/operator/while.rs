//! `While`: each sample holds until the next one, or until its timeout, within
//! a run of the extent.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{Pattern, ProcessError};
use crate::sequence_system::{Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence};

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::reading::{mismatch, read};

#[derive(Debug, Clone)]
pub(super) struct While {
    pattern: Pattern,
    timeout: Option<Timestamp>,
}

impl While {
    pub(super) const fn new(pattern: Pattern, timeout: Option<Timestamp>) -> Self {
        Self { pattern, timeout }
    }

    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::While(OnlineWhile {
            pattern: self.pattern.clone(),
            timeout: self.timeout,
            frontier: Timestamp::MIN,
            sample: None,
        })
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

/// The running state of [`While`]: the last sample, which holds into the next
/// chunk when the run of the extent does and its timeout has not passed.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlineWhile {
    pattern: Pattern,
    timeout: Option<Timestamp>,
    frontier: Timestamp,
    /// The timestamp of the last sample and whether it matches.
    sample: Option<(Timestamp, bool)>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineWhile {
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
        let (from, until) = (chunk.from(), chunk.to());
        let name = Attribute::name(format!("while({})", self.pattern));

        let mut spans = Vec::new();
        let mut known = Vec::new();
        let mut carried = self.sample;
        let mut last = None;
        let mut cursor = 0;

        // Nothing to read keeps the sample for the chunk that has something.
        if from < until {
            self.sample = None;
        }
        for run in observed.as_slice() {
            let (run_start, run_end) = run.bounds();

            while cursor < marks.len() && marks[cursor].0 < run_start {
                cursor += 1;
            }
            let first = cursor;
            while cursor < marks.len() && marks[cursor].0 < run_end {
                cursor += 1;
            }

            // The sample held into this run, if it is the one that continues
            // the run of the previous chunk, then the last point of each instant.
            let mut samples: Vec<(Timestamp, bool)> = if run_start == from {
                carried.take().into_iter().collect()
            } else {
                Vec::new()
            };
            for point in &marks[first..cursor] {
                match samples.last_mut() {
                    Some(sample) if sample.0 == point.0 => *sample = *point,
                    _ => samples.push(*point),
                }
            }

            for (index, &(timestamp, matched)) in samples.iter().enumerate() {
                let next = samples.get(index + 1).map_or(run_end, |next| next.0);
                let held = self
                    .timeout
                    .map_or(next, |timeout| next.min(timestamp.saturating_add(timeout)));
                // What lies before the run was written by an earlier chunk.
                let begin = timestamp.max(run_start);
                if begin < held {
                    known.push(TimeIntervalEvent::raw(begin, held));
                    if matched {
                        spans.push(TimeIntervalEvent::raw(begin, held));
                    }
                }
            }
            // The sample goes on only if the run does.
            last = if run_end == until {
                samples.last().copied()
            } else {
                None
            };
        }
        if from < until {
            self.sample = last;
        }
        self.frontier = until;

        Ok(Chunk::new(
            from,
            until,
            Sequence::TimeInterval(TimeIntervalSequence::from_parts(
                name,
                (0, 0),
                normalize(spans),
                Extent::from_normalized(normalize(known)),
            )),
        ))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}
