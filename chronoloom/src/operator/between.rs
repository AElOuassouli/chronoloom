//! `Between`: a state machine over the points of both patterns, run by run.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{OpenEnd, Pattern, ProcessError};
use crate::sequence_system::SequenceSystem;
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence};

use super::reading::read;

#[derive(Debug, Clone)]
pub(super) struct Between {
    start: Pattern,
    end: Pattern,
    open: OpenEnd,
}

impl Between {
    pub(super) const fn new(start: Pattern, end: Pattern, open: OpenEnd) -> Self {
        Self { start, end, open }
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let (start, end) = (&self.start, &self.end);
        let opening = read(start, inputs)?;
        let closing = read(end, inputs)?;

        // Each relevant point with whether it can open a span and whether it can
        // close one, in the order the state machine reads them.
        let (steps, observed, latest): (Vec<(Timestamp, bool, bool)>, Extent, Option<Timestamp>) =
            if start.key == end.key {
                let steps = opening
                    .points
                    .iter()
                    .zip(&closing.points)
                    .filter(|(opens, closes)| opens.1 || closes.1)
                    .map(|(opens, closes)| (opens.0, opens.1, closes.1))
                    .collect();
                let latest = opening.points.last().map(|point| point.0);

                (steps, opening.observed.clone(), latest)
            } else {
                // Stable, so at one instant the points of `start` come first.
                let mut steps: Vec<(Timestamp, bool, bool)> = opening
                    .points
                    .iter()
                    .filter(|point| point.1)
                    .map(|point| (point.0, true, false))
                    .chain(
                        closing
                            .points
                            .iter()
                            .filter(|point| point.1)
                            .map(|point| (point.0, false, true)),
                    )
                    .collect();
                steps.sort_by_key(|step| step.0);
                let latest = opening
                    .points
                    .last()
                    .map(|point| point.0)
                    .max(closing.points.last().map(|point| point.0));
                let observed =
                    Extent::from_normalized(opening.observed.intersection_spans(closing.observed));

                (steps, observed, latest)
            };

        let mut observed = observed;
        if self.open == OpenEnd::Head {
            // Unknown from the head on, so the run an open span ends at is cut
            // there. With no point at all nothing has been seen.
            let head = latest.map_or(Timestamp::MIN, |timestamp| timestamp.saturating_add(1));
            observed.narrow(&Extent::of_range(&(..head)));
        }

        let runs = observed.as_slice();
        let mut spans = Vec::new();
        let mut run = 0;
        let mut opened: Option<Timestamp> = None;

        for (timestamp, opens, closes) in steps {
            // Leaving a run abandons its state, closing a span left open.
            while run < runs.len() && runs[run].end() <= timestamp {
                if let Some(from) = opened.take() {
                    spans.push(TimeIntervalEvent::raw(from, runs[run].end()));
                }
                run += 1;
            }
            let Some(current) = runs.get(run) else { break };
            if timestamp < current.start() {
                continue;
            }

            match opened {
                // `timestamp` is inside the run, so below `i64::MAX`.
                None if opens => opened = Some(timestamp),
                Some(from) if closes => {
                    spans.push(TimeIntervalEvent::raw(from, timestamp.max(from + 1)));
                    opened = None;
                }
                _ => {}
            }
        }
        if let (Some(from), Some(last)) = (opened, runs.get(run)) {
            spans.push(TimeIntervalEvent::raw(from, last.end()));
        }

        Ok(TimeIntervalSequence::from_parts(
            Attribute::name(format!("between({start}, {end})")),
            (0, 0),
            normalize(spans),
            observed,
        ))
    }
}
