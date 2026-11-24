//! `Between`: a state machine over the points of both patterns, run by run.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{OpenEnd, Pattern, ProcessError};
use crate::sequence_system::{AnyPointSequence, Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence};

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::reading::{mismatch, read};

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

    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::Between(OnlineBetween {
            start: self.start.clone(),
            end: self.end.clone(),
            open: self.open,
            frontier: Timestamp::MIN,
            start_held: None,
            end_held: None,
            latest: None,
            opened: None,
        })
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

/// The running state of [`Between`].
///
/// It holds the input it has read but not yet made final (each member's chunks
/// from the output frontier on), the open start the state machine is in, and,
/// for [`OpenEnd::Head`], the latest point seen on either member.
///
/// The output is final up to the slowest input. With `Head` it also stops at the
/// head seen so far, since a later point moves the head and with it the end of
/// what was observed; once both inputs reach the end of time the head is known
/// and the rest is cut at it. A span still open at the end of a chunk is
/// written up to that end and continues in the next chunk while the run of the
/// extent does.
#[derive(Debug, Clone)]
pub(crate) struct OnlineBetween {
    start: Pattern,
    end: Pattern,
    open: OpenEnd,
    frontier: Timestamp,
    start_held: Option<Chunk>,
    /// Unused when both patterns read one member.
    end_held: Option<Chunk>,
    latest: Option<Timestamp>,
    opened: Option<Timestamp>,
}

impl OnlineBetween {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let same = self.start.key == self.end.key;
        Self::hold(inputs, &self.start, &mut self.start_held)?;
        if !same {
            Self::hold(inputs, &self.end, &mut self.end_held)?;
        }
        let start_held = self.start_held.as_ref().expect("held just above");
        let end_held = if same {
            start_held
        } else {
            self.end_held.as_ref().expect("held just above")
        };

        // Everything held, to know the latest point seen.
        let last_of = |held: &Chunk| {
            points_of(held)
                .instants_in(Timestamp::MIN, Timestamp::MAX)
                .last()
                .copied()
        };
        let seen = last_of(start_held).max(last_of(end_held));
        self.latest = self.latest.max(seen);

        let ready = start_held
            .to()
            .min(end_held.to())
            .min(to)
            .max(self.frontier);
        let head = self
            .latest
            .map_or(Timestamp::MIN, |timestamp| timestamp.saturating_add(1));
        // The head moves with every later point, so what lies beyond it is not
        // final until the inputs are.
        let limit = if self.open == OpenEnd::Head && ready != Timestamp::MAX {
            ready.min(head).max(self.frontier)
        } else {
            ready
        };

        let frontier = self.frontier;
        let start_part = start_held.slice(frontier, limit);
        let end_part = end_held.slice(frontier, limit);
        let (start_points, end_points) = (points_of(&start_part), points_of(&end_part));
        let opening = marks(&self.start, start_points)?;
        let closing = marks(&self.end, end_points)?;

        let start_rest = start_held.slice(limit, start_held.to());
        let end_rest = end_held.slice(limit, end_held.to());
        self.start_held = Some(start_rest);
        if !same {
            self.end_held = Some(end_rest);
        }
        self.frontier = limit;

        let name = Attribute::name(format!("between({}, {})", self.start, self.end));
        if limit == frontier {
            return Ok(Chunk::new(
                frontier,
                limit,
                Sequence::TimeInterval(TimeIntervalSequence::from_parts(
                    name,
                    (0, 0),
                    Vec::new(),
                    Extent::nothing(),
                )),
            ));
        }

        let (steps, mut observed): (Vec<(Timestamp, bool, bool)>, Extent) = if same {
            let steps = opening
                .iter()
                .zip(&closing)
                .filter(|(opens, closes)| opens.1 || closes.1)
                .map(|(opens, closes)| (opens.0, opens.1, closes.1))
                .collect();
            (steps, start_points.observed().clone())
        } else {
            let mut steps: Vec<(Timestamp, bool, bool)> = opening
                .iter()
                .filter(|point| point.1)
                .map(|point| (point.0, true, false))
                .chain(
                    closing
                        .iter()
                        .filter(|point| point.1)
                        .map(|point| (point.0, false, true)),
                )
                .collect();
            steps.sort_by_key(|step| step.0);
            let observed = Extent::from_normalized(
                start_points
                    .observed()
                    .intersection_spans(end_points.observed()),
            );
            (steps, observed)
        };
        if self.open == OpenEnd::Head {
            observed.narrow(&Extent::of_range(&(..head)));
        }

        let runs = observed.as_slice();
        // The open start carries over only into the run that continues it.
        let mut opened = self.opened.take();
        if runs.first().is_none_or(|run| run.start() > frontier) {
            opened = None;
        }
        let mut spans = Vec::new();
        let mut run = 0;
        // What lies before the frontier was written by an earlier chunk.
        let mut write = |from: Timestamp, until: Timestamp| {
            let from = from.max(frontier);
            if from < until {
                spans.push(TimeIntervalEvent::raw(from, until));
            }
        };

        for (timestamp, opens, closes) in steps {
            while run < runs.len() && runs[run].end() <= timestamp {
                if let Some(from) = opened.take() {
                    write(from, runs[run].end());
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
                    write(from, timestamp.max(from + 1));
                    opened = None;
                }
                _ => {}
            }
        }
        if let (Some(from), Some(last)) = (opened, runs.get(run)) {
            write(from, last.end());
            // The span goes on only if the run does.
            if last.end() == limit {
                self.opened = Some(from);
            }
        }

        Ok(Chunk::new(
            frontier,
            limit,
            Sequence::TimeInterval(TimeIntervalSequence::from_parts(
                name,
                (0, 0),
                normalize(spans),
                observed,
            )),
        ))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }

    /// Join the new chunk of the member `pattern` reads to what is held of it.
    fn hold(
        inputs: &Inputs,
        pattern: &Pattern,
        held: &mut Option<Chunk>,
    ) -> Result<(), ProcessError> {
        let chunk = inputs
            .get(&pattern.key)
            .ok_or_else(|| ProcessError::UnknownKey {
                key: pattern.key.clone(),
            })?;
        if !matches!(chunk.sequence(), Sequence::TimePoint(_)) {
            return Err(ProcessError::NotPoints {
                key: pattern.key.clone(),
            });
        }

        *held = Some(match held.take() {
            Some(earlier) => earlier.join(chunk.clone()),
            None => chunk.clone(),
        });
        Ok(())
    }
}

/// The points of a chunk that `hold` checked.
fn points_of(chunk: &Chunk) -> &AnyPointSequence {
    match chunk.sequence() {
        Sequence::TimePoint(points) => points,
        Sequence::TimeInterval(_) => unreachable!("`hold` refuses chunks that are not points"),
    }
}

/// Whether each point matches `pattern`, in order.
fn marks(
    pattern: &Pattern,
    points: &AnyPointSequence,
) -> Result<Vec<(Timestamp, bool)>, ProcessError> {
    pattern
        .condition
        .marks(points)
        .ok_or_else(|| mismatch(pattern, points))
}
