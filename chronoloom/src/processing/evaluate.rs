//! Evaluating formulas against a system: the meaning of each node.

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::sequence_system::{AnyPointSequence, Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};

use super::condition::Pattern;
use super::edge::Edge;
use super::errors::ProcessError;
use super::events::Events;
use super::formula::{Formula, OpenEnd};

/// Whether each point of a member matches, in the member's order, and where it
/// was observed.
struct Marks<'a> {
    /// Each point's timestamp, and whether its value matches.
    points: Vec<(Timestamp, bool)>,
    /// The instants the member was observed over.
    observed: &'a Extent,
}

/// The formula's timeline, labelled with the formula written out.
pub(super) fn evaluate(
    formula: &Formula,
    system: &SequenceSystem,
) -> Result<TimeIntervalSequence, ProcessError> {
    let mut timeline = node(formula, system)?;
    timeline.set_attribute(formula.to_string());

    Ok(timeline)
}

/// The sequence of an events formula, labelled with the formula written out.
pub(super) fn evaluate_events(
    events: &Events,
    system: &SequenceSystem,
) -> Result<Sequence, ProcessError> {
    let label = events.to_string();

    match events {
        Events::Edges(formula) => {
            let timeline = evaluate(formula, system)?;
            let edges = timeline
                .iter()
                .flat_map(|span| {
                    [
                        TimePointEvent::new(span.start(), Edge::Start),
                        TimePointEvent::new(span.end(), Edge::End),
                    ]
                })
                .collect();

            Ok(Sequence::from(TimePointSequence::from_parts(
                Attribute::name(label),
                edges,
                timeline.observed().clone(),
            )))
        }
        Events::Matching(pattern) => {
            let points = points_of(pattern, system)?;
            let mut matching = pattern
                .condition
                .matching(points)
                .ok_or_else(|| mismatch(pattern, points))?;
            matching.set_attribute(label);

            Ok(Sequence::from(matching))
        }
    }
}

/// The timeline of one node of the tree.
fn node(formula: &Formula, system: &SequenceSystem) -> Result<TimeIntervalSequence, ProcessError> {
    match formula {
        Formula::State(key) => match system.get(key.as_str()) {
            Some(Sequence::TimeInterval(timeline)) => Ok(timeline.clone()),
            Some(Sequence::TimePoint(_)) => Err(ProcessError::NotATimeline { key: key.clone() }),
            None => Err(ProcessError::UnknownKey { key: key.clone() }),
        },
        Formula::Between { start, end, open } => between(start, end, *open, system),
        Formula::While(pattern) => hold(pattern, system),
        Formula::Occurs(pattern) => occurs(pattern, system),
        Formula::Not(inner) => Ok(node(inner, system)?.complement()),
        Formula::And(formulas) => {
            let mut parts = formulas.iter();
            let Some(first) = parts.next() else {
                // The identity of the intersection: active everywhere.
                let everywhere = TimeIntervalEvent::raw(Timestamp::MIN, Timestamp::MAX);
                return Ok(TimeIntervalSequence::from_spans("⊤", vec![everywhere]));
            };

            let mut timeline = node(first, system)?;
            for part in parts {
                timeline = timeline.intersection(&node(part, system)?);
            }
            Ok(timeline)
        }
        Formula::Or(formulas) => {
            let mut parts = formulas.iter();
            let Some(first) = parts.next() else {
                // The identity of the union: active nowhere.
                return Ok(TimeIntervalSequence::new("⊥"));
            };

            let mut timeline = node(first, system)?;
            for part in parts {
                timeline = timeline.union(&node(part, system)?);
            }
            Ok(timeline)
        }
        Formula::Shift {
            formula,
            alpha,
            beta,
        } => Ok(node(formula, system)?.transform_unbounded(*alpha, *beta)),
    }
}

/// The point sequence a pattern reads.
fn points_of<'a>(
    pattern: &Pattern,
    system: &'a SequenceSystem,
) -> Result<&'a AnyPointSequence, ProcessError> {
    match system.get(pattern.key.as_str()) {
        Some(Sequence::TimePoint(points)) => Ok(points),
        Some(Sequence::TimeInterval(_)) => Err(ProcessError::NotPoints {
            key: pattern.key.clone(),
        }),
        None => Err(ProcessError::UnknownKey {
            key: pattern.key.clone(),
        }),
    }
}

/// The error for a pattern whose condition is over another value type than
/// `points` carries.
fn mismatch(pattern: &Pattern, points: &AnyPointSequence) -> ProcessError {
    ProcessError::ValueTypeMismatch {
        key: pattern.key.clone(),
        expected: points.value_type_name(),
        found: pattern.condition.value_type_name(),
    }
}

/// Which points of the member a pattern reads match, and where it was observed.
fn read<'a>(pattern: &Pattern, system: &'a SequenceSystem) -> Result<Marks<'a>, ProcessError> {
    let points = points_of(pattern, system)?;
    let marks = pattern
        .condition
        .marks(points)
        .ok_or_else(|| mismatch(pattern, points))?;

    Ok(Marks {
        points: marks,
        observed: points.observed(),
    })
}

/// `Occurs`: `[t, t + 1)` for each matching point inside the extent.
fn occurs(
    pattern: &Pattern,
    system: &SequenceSystem,
) -> Result<TimeIntervalSequence, ProcessError> {
    let marks = read(pattern, system)?;
    let spans = marks
        .points
        .iter()
        // An instant the extent contains is below `i64::MAX`, so `t + 1` holds.
        .filter(|(timestamp, matched)| *matched && marks.observed.contains(*timestamp))
        .map(|(timestamp, _)| TimeIntervalEvent::raw(*timestamp, *timestamp + 1))
        .collect();

    Ok(TimeIntervalSequence::from_parts(
        Attribute::name(format!("occurs({pattern})")),
        (0, 0),
        normalize(spans),
        marks.observed.clone(),
    ))
}

/// `While`: each sample holds until the next one, within a run of the extent.
fn hold(pattern: &Pattern, system: &SequenceSystem) -> Result<TimeIntervalSequence, ProcessError> {
    let marks = read(pattern, system)?;
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
        let Some(&(first_sample, _)) = samples.first() else {
            continue;
        };
        known.push(TimeIntervalEvent::raw(first_sample, run_end));

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

            if samples[last].1 {
                let until = samples.get(last + 1).map_or(run_end, |next| next.0);
                spans.push(TimeIntervalEvent::raw(timestamp, until));
            }
            index = last + 1;
        }
    }

    Ok(TimeIntervalSequence::from_parts(
        Attribute::name(format!("while({pattern})")),
        (0, 0),
        normalize(spans),
        Extent::from_normalized(known),
    ))
}

/// `Between`: a state machine over the points of both patterns, run by run.
fn between(
    start: &Pattern,
    end: &Pattern,
    open: OpenEnd,
    system: &SequenceSystem,
) -> Result<TimeIntervalSequence, ProcessError> {
    let opening = read(start, system)?;
    let closing = read(end, system)?;

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
    if open == OpenEnd::Head {
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
