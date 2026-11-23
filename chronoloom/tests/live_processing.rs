//! Processors run online: what a `LiveProcessor` emits is the batch result,
//! below the frontier of each output, whatever the order and the watermarks the
//! inputs arrive with.

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;
#[path = "common/update.rs"]
mod update;

use std::collections::BTreeMap;

use chronoloom::{
    AnyValue, Condition, Edge, Emission, Emitted, Events, Formula, Key, LiveProcessor, OpenEnd,
    Pattern, ProcessError, Processor, Recipe, SequenceSystem, StreamSystem, TimeIntervalEvent,
    TimeIntervalSequence, TimePointEvent, TimePointSequence, Value,
};

use update::Evaluate;

use common::{bounds, for_each_seed, span};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

const CASES: u64 = 500;

// ---------------------------------------------------------------------------
// Random processors
// ---------------------------------------------------------------------------

/// The formula with every head replaced by the end of the extent: what the
/// batch evaluation must be compared with, since online the head is the
/// horizon.
fn headless(formula: &Formula) -> Formula {
    match formula {
        Formula::Between { start, end, .. } => Formula::between(start.clone(), end.clone()),
        Formula::Not(inner) => !headless(inner),
        Formula::And(parts) => Formula::And(parts.iter().map(headless).collect()),
        Formula::Or(parts) => Formula::Or(parts.iter().map(headless).collect()),
        Formula::Shift {
            formula,
            alpha,
            beta,
        } => headless(formula).shift(*alpha, *beta),
        leaf => leaf.clone(),
    }
}

fn reference_processor(processor: &[Processor]) -> Vec<Processor> {
    processor
        .iter()
        .map(|processor| {
            let (key, output) = (processor.output(), processor.recipe());
            let output: Recipe = match output {
                Recipe::Timeline(formula) => headless(formula).into(),
                Recipe::Points(Events::Edges(formula)) => Events::Edges(headless(formula)).into(),
                points @ Recipe::Points(
                    Events::Matching(_) | Events::Project { .. } | Events::Changes { .. },
                ) => points.clone(),
                Recipe::PerEntity { by, formula } => {
                    Recipe::per_entity(by.clone(), headless(formula))
                }
                Recipe::AnyEntity { by, formula } => {
                    Recipe::any_entity(by.clone(), headless(formula))
                }
                Recipe::CountEntities { by, formula } => {
                    Recipe::count_entities(by.clone(), headless(formula))
                }
            };
            Processor::new(key.clone(), output)
        })
        .collect()
}

/// The lag rule, written again from its statement, for a formula.
fn lag(formula: &Formula, marks: &BTreeMap<String, i64>) -> i64 {
    let at = |key: &chronoloom::Key| marks[key.as_str()];
    match formula {
        Formula::State(key) => at(key),
        Formula::Between { start, end, .. } => at(start.key()).min(at(end.key())),
        Formula::While { pattern, .. } | Formula::Occurs(pattern) => at(pattern.key()),
        Formula::Not(inner) => lag(inner, marks),
        Formula::And(parts) | Formula::Or(parts) => parts
            .iter()
            .map(|part| lag(part, marks))
            .min()
            .unwrap_or(i64::MAX),
        Formula::Shift {
            formula,
            alpha,
            beta,
        } => match lag(formula, marks) {
            i64::MAX => i64::MAX,
            inner => inner.saturating_add(*alpha.min(beta)),
        },
    }
}

// ---------------------------------------------------------------------------
// Random streams
// ---------------------------------------------------------------------------

/// The batch inputs for a stream: everything recorded, observed up to the
/// horizon.
fn batch_inputs(stream: &Stream) -> SequenceSystem {
    let observed =
        chronoloom::Extent::from_spans(vec![
            TimeIntervalEvent::span(i64::MIN, stream.horizon).expect("ordered")
        ]);

    let mut system = SequenceSystem::new();
    for key in POINT_INPUTS {
        let events = stream
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Point(at, ..) if false => unreachable!("{at}"),
                Op::Point(on, at, value) if *on == key => Some(TimePointEvent::new(*at, *value)),
                _ => None,
            })
            .collect();
        let points = TimePointSequence::from_events(key, events).with_observed(observed.clone());
        system.insert(key, points);
    }
    for key in RECORD_INPUTS {
        let events = stream
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Record(on, at, value) if *on == key => {
                    Some(TimePointEvent::new(*at, value.clone()))
                }
                _ => None,
            })
            .collect();
        let records = TimePointSequence::from_events(key, events).with_observed(observed.clone());
        system.insert(key, records);
    }
    for key in STATE_INPUTS {
        let recorded = stream
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Span(on, start, end) if *on == key => Some(span(*start, *end)),
                _ => None,
            })
            .collect();
        let timeline =
            TimeIntervalSequence::from_spans(key, recorded).with_observed(observed.clone());
        system.insert(key, timeline);
    }

    system
}

// ---------------------------------------------------------------------------
// Replaying, and what the batch says
// ---------------------------------------------------------------------------

/// What a test watches of a replay: the processor, the watermarks and what the
/// last call returned.
type Observer<'a> = dyn FnMut(&LiveProcessor, &BTreeMap<String, i64>, &[Emission]) + 'a;

/// Feed the stream, then finish, calling `observe` after each call that returns
/// emissions with the live processor, the watermarks, and what the call
/// returned.
fn replay(processor: &[Processor], stream: &Stream, observe: &mut Observer) -> Vec<Emission> {
    let mut live = LiveProcessor::new(processor, stream.from).expect("a valid processor");
    let mut marks: BTreeMap<String, i64> = POINT_INPUTS
        .iter()
        .chain(&STATE_INPUTS)
        .chain(&RECORD_INPUTS)
        .map(|key| ((*key).to_owned(), stream.from))
        .collect();
    let mut all = Vec::new();

    for op in &stream.ops {
        let outcome = match op {
            Op::Point(key, at, value) => live
                .on_point(key, TimePointEvent::new(*at, *value))
                .map(|()| None),
            Op::Record(key, at, value) => live
                .on_point(key, TimePointEvent::new(*at, value.clone()))
                .map(|()| None),
            Op::Span(key, start, end) => live.on_span(key, span(*start, *end)).map(|()| None),
            Op::Advance(key, to) => live.advance(key, *to).map(|emitted| {
                marks.insert((*key).to_owned(), *to);
                Some(emitted)
            }),
        };
        match outcome {
            Ok(Some(emitted)) => {
                observe(&live, &marks, &emitted);
                all.extend(emitted);
            }
            Ok(None) => {}
            // An input that no output reads is not one of the processor's.
            Err(ProcessError::UnknownKey { .. }) => {}
            Err(error) => panic!("{op:?}: {error}"),
        }
    }

    let horizon = marks
        .values()
        .copied()
        .max()
        .unwrap_or(stream.from)
        .max(stream.horizon);
    for mark in marks.values_mut() {
        *mark = (*mark).max(horizon);
    }
    let emitted = live.finish(stream.horizon).expect("finishing cannot fail");
    all.extend(emitted);

    all
}

/// What one output is in the batch result: its edges or its matching values,
/// as `(instant, tag)` with the tag `0` for a start, `1` for an end, and the
/// value for a matching point.
fn expected(reference: &StreamSystem, key: &str, output: &Recipe) -> Vec<(i64, i64)> {
    match output {
        Recipe::PerEntity { .. } => unreachable!("a group has a member per entity"),
        Recipe::Timeline(_) | Recipe::AnyEntity { .. } => reference
            .intervals(key)
            .expect("a state output is a timeline")
            .iter()
            .flat_map(|span| [(span.start(), 0), (span.end(), 1)])
            .collect(),
        Recipe::Points(Events::Edges(_)) => reference
            .points::<Edge>(key)
            .expect("an edges output is edges")
            .iter()
            .map(|event| (event.timestamp(), i64::from(*event.value() == Edge::End)))
            .collect(),
        Recipe::Points(Events::Matching(_) | Events::Project { .. } | Events::Changes { .. })
        | Recipe::CountEntities { .. } => point_tags(reference, key),
    }
}

/// The members of the per-entity output `key` in the batch result.
fn members_of(reference: &StreamSystem, key: &Key) -> Vec<Key> {
    let prefix = format!("{key}/");
    reference
        .keys()
        .filter(|member| member.as_str().starts_with(&prefix))
        .cloned()
        .collect()
}

/// A value of whatever type a point output holds, as a number: an integer is
/// itself, a flag is 0 or 1, a float is its whole part, and a record is its
/// field `n`, or -1 without one.
fn tag_value(value: &Value) -> i64 {
    match value {
        Value::Integer(number) => *number,
        Value::Bool(flag) => i64::from(*flag),
        Value::Float(number) => *number as i64,
        Value::Record(_) => value.get("n").and_then(Value::as_integer).unwrap_or(-1),
        Value::Null | Value::Text(_) => -1,
    }
}

/// The points of a point output of the batch result, as `(instant, tag)`.
fn point_tags(reference: &StreamSystem, key: &str) -> Vec<(i64, i64)> {
    if let Some(points) = reference.points::<i64>(key) {
        points
            .iter()
            .map(|event| (event.timestamp(), *event.value()))
            .collect()
    } else if let Some(points) = reference.points::<f64>(key) {
        points
            .iter()
            .map(|event| (event.timestamp(), *event.value() as i64))
            .collect()
    } else if let Some(points) = reference.points::<bool>(key) {
        points
            .iter()
            .map(|event| (event.timestamp(), i64::from(*event.value())))
            .collect()
    } else {
        reference
            .points::<Value>(key)
            .expect("a point output is numbers, flags or records")
            .iter()
            .map(|event| (event.timestamp(), tag_value(event.value())))
            .collect()
    }
}

/// The tag of an emitted point, as `point_tags` gives the batch point's.
fn tag_emitted(value: &AnyValue) -> i64 {
    if let Some(number) = value.downcast_ref::<i64>() {
        *number
    } else if let Some(number) = value.downcast_ref::<f64>() {
        *number as i64
    } else if let Some(flag) = value.downcast_ref::<bool>() {
        i64::from(*flag)
    } else {
        tag_value(
            value
                .downcast_ref::<Value>()
                .expect("numbers, flags or records"),
        )
    }
}

fn tagged(emission: &Emission) -> (i64, i64) {
    match &emission.item {
        Emitted::Edge(edge) => (edge.timestamp(), i64::from(*edge.value() == Edge::End)),
        Emitted::Point(point) => (point.timestamp(), tag_emitted(point.value())),
    }
}

fn emitted_for(emissions: &[Emission], key: &str) -> Vec<(i64, i64)> {
    emissions
        .iter()
        .filter(|emission| emission.key.as_str() == key)
        .map(tagged)
        .collect()
}

#[test]
fn online_equals_batch_below_every_frontier_and_in_the_end() {
    for_each_seed(CASES, |random, _| {
        let processor = processors(random);
        let stream = stream(random);
        let reference = update::run(
            &update::streams_of(&batch_inputs(&stream)),
            &reference_processor(&processor),
        )
        .expect("the batch result");

        let fed = stream
            .ops
            .iter()
            .filter(|op| !matches!(op, Op::Advance(..)))
            .count();
        let mut so_far: Vec<Emission> = Vec::new();
        let mut frontiers: BTreeMap<String, i64> = BTreeMap::new();

        let all = replay(&processor, &stream, &mut |live, marks, emitted| {
            so_far.extend(emitted.iter().cloned());

            // Instants of a call come in order, and never before the last call's.
            assert!(emitted.windows(2).all(|pair| {
                (pair[0].timestamp(), &pair[0].key) <= (pair[1].timestamp(), &pair[1].key)
            }));
            // A record is held once as fed and once in the view of its entity.
            assert!(live.retained() <= 2 * fed, "never more than was fed");

            let mut outputs: BTreeMap<String, i64> = BTreeMap::new();
            for (key, output) in processor.iter().map(|p| (p.output(), p.recipe())) {
                let frontier = live.frontier(key.as_str()).expect("an output");
                if let Some(&before) = frontiers.get(key.as_str()) {
                    assert!(frontier >= before, "{key}: the frontier went back");
                }
                frontiers.insert(key.as_str().to_owned(), frontier);

                if let Recipe::PerEntity { by, formula } = output {
                    for member in members_of(&reference, key) {
                        let items = expected(
                            &reference,
                            member.as_str(),
                            &Recipe::Timeline(formula.clone()),
                        );
                        match live.frontier(member.as_str()) {
                            Some(member_frontier) => {
                                let wanted: Vec<(i64, i64)> = items
                                    .into_iter()
                                    .filter(|item| item.0 < member_frontier)
                                    .collect();
                                assert_eq!(
                                    emitted_for(&so_far, member.as_str()),
                                    wanted,
                                    "{member} below {member_frontier}"
                                );
                                let mut all = marks.clone();
                                all.extend(outputs.clone());
                                assert_eq!(
                                    member_frontier,
                                    lag(formula, &all),
                                    "{member}: the lag rule"
                                );
                            }
                            // Not seen yet: all its records are to come, and it
                            // exists from the first of them.
                            None => assert!(
                                items.iter().all(|item| item.0 >= marks[by.key.as_str()]),
                                "{member} has edges before its first record"
                            ),
                        }
                    }
                    outputs.insert(key.as_str().to_owned(), frontier);
                    continue;
                }
                if let Recipe::AnyEntity { by, .. } | Recipe::CountEntities { by, .. } = output {
                    assert!(
                        frontier <= marks[by.key.as_str()],
                        "{key}: no further than its records"
                    );
                    outputs.insert(key.as_str().to_owned(), frontier);
                    let wanted: Vec<(i64, i64)> = expected(&reference, key.as_str(), output)
                        .into_iter()
                        .filter(|item| item.0 < frontier)
                        .collect();
                    assert_eq!(
                        emitted_for(&so_far, key.as_str()),
                        wanted,
                        "{key} below {frontier}"
                    );
                    continue;
                }

                let sequence_lag = match output {
                    Recipe::PerEntity { .. }
                    | Recipe::AnyEntity { .. }
                    | Recipe::CountEntities { .. } => {
                        unreachable!("handled above")
                    }
                    Recipe::Timeline(formula) | Recipe::Points(Events::Edges(formula)) => {
                        lag(formula, &{
                            let mut all = marks.clone();
                            all.extend(outputs.clone());
                            all
                        })
                    }
                    Recipe::Points(Events::Matching(read)) => {
                        let mut all = marks.clone();
                        all.extend(outputs.clone());
                        all[read.key().as_str()]
                    }
                    Recipe::Points(
                        Events::Project { key: read, .. } | Events::Changes { key: read },
                    ) => {
                        let mut all = marks.clone();
                        all.extend(outputs.clone());
                        all[read.as_str()]
                    }
                };
                assert_eq!(frontier, sequence_lag, "{key}: the lag rule");
                outputs.insert(key.as_str().to_owned(), frontier);

                let wanted: Vec<(i64, i64)> = expected(&reference, key.as_str(), output)
                    .into_iter()
                    .filter(|item| item.0 < frontier)
                    .collect();
                assert_eq!(
                    emitted_for(&so_far, key.as_str()),
                    wanted,
                    "{key} below {frontier} {:?} {:?}",
                    processor
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    stream.ops
                );
            }
        });

        // Running again gives the same emissions, instant by instant per output.
        assert_eq!(all, replay(&processor, &stream, &mut |_, _, _| {}));

        for (key, output) in processor.iter().map(|p| (p.output(), p.recipe())) {
            if let Recipe::PerEntity { formula, .. } = output {
                let members = members_of(&reference, key);
                let prefix = format!("{key}/");
                for emission in &all {
                    if emission.key.as_str().starts_with(&prefix) {
                        assert!(
                            members.contains(&emission.key),
                            "{}: not a member",
                            emission.key
                        );
                    }
                }
                for member in members {
                    assert_eq!(
                        emitted_for(&all, member.as_str()),
                        expected(
                            &reference,
                            member.as_str(),
                            &Recipe::Timeline(formula.clone())
                        ),
                        "{member} in the end"
                    );
                }
                continue;
            }
            assert_eq!(
                emitted_for(&all, key.as_str()),
                expected(&reference, key.as_str(), output),
                "{key} in the end"
            );
            let instants: Vec<i64> = all
                .iter()
                .filter(|emission| emission.key == *key)
                .map(Emission::timestamp)
                .collect();
            assert!(instants.windows(2).all(|pair| pair[0] <= pair[1]));

            // Edges rebuilt into spans are the batch timeline.
            if let Recipe::Timeline(_) = output {
                let edges: Vec<TimePointEvent<Edge>> = all
                    .iter()
                    .filter(|emission| emission.key == *key)
                    .filter_map(|emission| match &emission.item {
                        Emitted::Edge(edge) => Some(*edge),
                        Emitted::Point(_) => None,
                    })
                    .collect();
                let mut rebuilt = StreamSystem::new();
                rebuilt
                    .insert_stream("edges", TimePointSequence::from_events("edges", edges))
                    .unwrap();
                let between = Formula::between(
                    Pattern::new("edges", Condition::Equals(Edge::Start)),
                    Pattern::new("edges", Condition::Equals(Edge::End)),
                );
                let timeline = between.evaluate_over(&rebuilt).expect("edges are edges");
                let batch = reference.intervals(key.as_str()).expect("a timeline");
                assert_eq!(bounds(&timeline), bounds(batch), "{key} rebuilt");
            }
        }
        for key in processor.iter().map(Processor::output) {
            assert_eq!(
                processor_frontier_after_finish(&processor, &stream, key.as_str()),
                Some(i64::MAX)
            );
        }
    });
}

fn processor_frontier_after_finish(
    processor: &[Processor],
    stream: &Stream,
    key: &str,
) -> Option<i64> {
    let mut live = LiveProcessor::new(processor, stream.from).expect("valid");
    live.finish(stream.horizon).expect("finishing");
    live.frontier(key)
}

// ---------------------------------------------------------------------------
// Examples and edge cases
// ---------------------------------------------------------------------------

fn busy() -> Vec<Processor> {
    vec![Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64))),
    )]
}

fn edges(emitted: &[Emission]) -> Vec<(i64, Edge)> {
    emitted
        .iter()
        .map(|emission| match &emission.item {
            Emitted::Edge(edge) => (edge.timestamp(), *edge.value()),
            Emitted::Point(_) => panic!("an edge was expected"),
        })
        .collect()
}

#[test]
fn a_start_is_emitted_before_its_span_closes_and_finish_closes_it() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.on_point("count", TimePointEvent::new(7, 9_i64))
        .expect("on time");

    assert_eq!(
        edges(&live.advance("count", 10).expect("known")),
        [(7, Edge::Start)]
    );
    assert_eq!(live.frontier("busy"), Some(10));
    assert!(live.advance("count", 20).expect("known").is_empty());
    assert_eq!(edges(&live.finish(30).expect("done")), [(30, Edge::End)]);
    assert_eq!(live.frontier("busy"), Some(i64::MAX));
    assert!(live.finish(40).expect("done").is_empty());
}

#[test]
fn nothing_is_emitted_for_what_is_unknown() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.on_point("count", TimePointEvent::new(4, 1_i64))
        .expect("on time");

    // Active nowhere and unknown before the first sample: no span, no edge.
    assert!(live.advance("count", 50).expect("known").is_empty());
    assert!(live.finish(60).expect("done").is_empty());
}

#[test]
fn a_shift_lags_its_input_by_its_smaller_bound() {
    let shifted = Formula::occurs(Pattern::new("door", Condition::Any::<i64>));
    let processor = vec![
        Processor::new("early", shifted.clone().shift(-2, 2)),
        Processor::new("late", shifted.shift(3, 5)),
    ];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");

    live.advance("door", 10).expect("known");
    assert_eq!(live.frontier("early"), Some(8));
    assert_eq!(live.frontier("late"), Some(13));
}

#[test]
fn chained_outputs_chain_their_lags() {
    let processor = vec![
        Processor::new(
            "first",
            Formula::occurs(Pattern::new("door", Condition::Any::<i64>)).shift(-1, -1),
        ),
        Processor::new("second", Formula::state("first").shift(-2, -2)),
    ];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");

    live.on_point("door", TimePointEvent::new(5, 1_i64))
        .expect("on time");
    let emitted = live.advance("door", 10).expect("known");
    assert_eq!(live.frontier("second"), Some(7));
    // `second` is `[5, 6)` moved back three ticks.
    let second: Vec<_> = emitted
        .iter()
        .filter(|e| e.key.as_str() == "second")
        .collect();
    assert_eq!(
        edges(&second.iter().map(|e| (*e).clone()).collect::<Vec<_>>()),
        [(2, Edge::Start), (3, Edge::End),]
    );
}

#[test]
fn matching_emits_the_values_of_the_member() {
    let processor = vec![Processor::new(
        "high",
        Events::Matching(Pattern::new("reading", Condition::Greater(1.5_f64))),
    )];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    for (at, value) in [(1, 1.0), (2, 2.0), (3, 3.0)] {
        live.on_point("reading", TimePointEvent::new(at, value))
            .expect("on time");
    }

    let emitted = live.advance("reading", 3).expect("known");
    assert_eq!(emitted.len(), 1, "the point at the frontier is not final");
    assert_eq!(
        emitted[0],
        Emission {
            key: "high".into(),
            item: Emitted::Point(TimePointEvent::new(2, AnyValue::new(2.0_f64))),
        }
    );
    assert_eq!(live.finish(4).expect("done").len(), 1);
}

#[test]
fn head_is_the_frontier_online() {
    let processor = vec![Processor::new(
        "open",
        Formula::between_with(
            Pattern::new("motion", Condition::Equals(1_i64)),
            Pattern::new("motion", Condition::Equals(0_i64)),
            OpenEnd::Head,
        ),
    )];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    live.on_point("motion", TimePointEvent::new(5, 1_i64))
        .expect("on time");

    assert_eq!(
        edges(&live.advance("motion", 20).expect("known")),
        [(5, Edge::Start)]
    );
    assert_eq!(edges(&live.finish(25).expect("done")), [(25, Edge::End)]);
}

#[test]
fn late_unknown_and_mistyped_events_are_refused_and_change_nothing() {
    let mut live = LiveProcessor::new(&busy(), 10).expect("valid");

    assert_eq!(
        live.on_point("count", TimePointEvent::new(3, 1_i64)),
        Err(ProcessError::Late {
            key: "count".into(),
            timestamp: 3,
            frontier: 10
        })
    );
    assert!(matches!(
        live.on_point("count", TimePointEvent::new(11, 1.5_f64)),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    assert!(matches!(
        live.on_point("nothing", TimePointEvent::new(11, 1_i64)),
        Err(ProcessError::UnknownKey { .. })
    ));
    assert!(matches!(
        live.on_span("count", span(11, 12)),
        Err(ProcessError::NotATimeline { .. })
    ));
    assert!(matches!(
        live.advance("busy", 20),
        Err(ProcessError::UnknownKey { .. })
    ));
    assert_eq!(live.retained(), 0);

    live.finish(20).expect("done");
    assert_eq!(
        live.on_point("count", TimePointEvent::new(30, 1_i64)),
        Err(ProcessError::Finished)
    );
}

#[test]
fn a_timeline_input_takes_spans() {
    let processor = vec![Processor::new("away", !Formula::state("home"))];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");

    assert!(matches!(
        live.on_point("home", TimePointEvent::new(1, 1_i64)),
        Err(ProcessError::NotPoints { .. })
    ));
    live.on_span("home", span(2, 5)).expect("on time");
    let emitted = live.advance("home", 10).expect("known");
    assert_eq!(
        edges(&emitted),
        [(i64::MIN, Edge::Start), (2, Edge::End), (5, Edge::Start)]
    );
}

#[test]
fn a_processor_that_cannot_run_is_refused_when_built() {
    let cycle = vec![
        Processor::new("a", Formula::state("b")),
        Processor::new("b", Formula::state("a")),
    ];
    assert!(matches!(
        LiveProcessor::new(&cycle, 0),
        Err(ProcessError::Cycle { .. })
    ));

    let clash = vec![Processor::new(
        "x",
        Formula::state("m").and(Formula::occurs(Pattern::new("m", Condition::Any::<i64>))),
    )];
    assert!(matches!(
        LiveProcessor::new(&clash, 0),
        Err(ProcessError::NotPoints { .. })
    ));

    let types = vec![Processor::new(
        "x",
        Formula::occurs(Pattern::new("m", Condition::Any::<i64>))
            .and(Formula::occurs(Pattern::new("m", Condition::Any::<f64>))),
    )];
    assert!(matches!(
        LiveProcessor::new(&types, 0),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));

    let fields = vec![Processor::new(
        "x",
        Formula::occurs(Pattern::field("m", "n", Condition::Any))
            .and(Formula::occurs(Pattern::new("m", Condition::Any::<i64>))),
    )];
    assert!(matches!(
        LiveProcessor::new(&fields, 0),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));

    let chained = vec![
        Processor::new("edges", Events::Edges(Formula::state("up"))),
        Processor::new(
            "x",
            Formula::occurs(Pattern::new("edges", Condition::Any::<i64>)),
        ),
    ];
    assert!(matches!(
        LiveProcessor::new(&chained, 0),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
}

// ---------------------------------------------------------------------------
// The state bound
// ---------------------------------------------------------------------------

#[test]
fn the_retained_window_stays_small_on_a_long_stream() {
    let count = |at: i64| Pattern::new("count", Condition::Greater(at));
    let processor = vec![
        Processor::new("busy", Formula::while_matching(count(5_i64))),
        Processor::new(
            "spike",
            Events::Edges(Formula::occurs(count(8_i64)).shift(-2, 5)),
        ),
        Processor::new(
            "episode",
            Formula::between(
                Pattern::new("count", Condition::Greater(8_i64)),
                Pattern::new("count", Condition::Less(3_i64)),
            ),
        ),
        Processor::new("calm", !Formula::state("busy")),
    ];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");

    let mut worst = 0;
    for tick in 0..3_000_i64 {
        let value = (tick * 7 + tick / 13) % 11;
        live.on_point("count", TimePointEvent::new(tick, value))
            .expect("on time");
        live.advance("count", tick + 1).expect("known");
        worst = worst.max(live.retained());
    }

    // Reach 5, and the span of an episode lasts a few ticks: a few dozen events.
    assert!(worst < 40, "retained {worst} events");
    assert_eq!(live.frontier("busy"), Some(3_000));
}

#[test]
fn a_long_open_span_keeps_what_it_needs_and_no_more() {
    let processor = vec![Processor::new(
        "held",
        Formula::while_matching(Pattern::new("mode", Condition::Equals(1_i64))),
    )];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    live.on_point("mode", TimePointEvent::new(1, 1_i64))
        .expect("on time");
    let mut emitted = Vec::new();
    for tick in 2..2_000 {
        emitted.extend(live.advance("mode", tick).expect("known"));
        assert!(
            live.retained() <= 1,
            "the sample that holds the state is kept"
        );
    }
    live.on_point("mode", TimePointEvent::new(2_000, 0_i64))
        .expect("on time");
    emitted.extend(live.advance("mode", 2_001).expect("known"));
    assert_eq!(edges(&emitted), [(1, Edge::Start), (2_000, Edge::End)]);
}

#[test]
fn point_events_and_the_bound_agree_with_the_batch_on_a_long_stream() {
    let processor = vec![Processor::new(
        "run",
        Formula::between(
            Pattern::new("signal", Condition::Equals(1_i64)),
            Pattern::new("signal", Condition::Equals(0_i64)),
        ),
    )];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    let mut batch = TimePointSequence::<i64>::new("signal");
    let mut all = Vec::new();
    for tick in 0..1_000_i64 {
        let value = i64::from((tick * tick / 7) % 3 == 0);
        batch.insert(TimePointEvent::new(tick, value));
        live.on_point("signal", TimePointEvent::new(tick, value))
            .expect("on time");
        all.extend(live.advance("signal", tick + 1).expect("known"));
        assert!(live.retained() < 60);
    }
    all.extend(live.finish(1_000).expect("done"));

    let mut system = SequenceSystem::new();
    system.insert(
        "signal",
        batch.with_observed(chronoloom::Extent::from_spans(vec![
            TimeIntervalEvent::span(i64::MIN, 1_000).expect("ordered"),
        ])),
    );
    let expected = update::run(&update::streams_of(&system), &processor).expect("batch");
    let spans: Vec<(i64, i64)> = bounds(expected.intervals("run").expect("timeline"));
    let rebuilt: Vec<(i64, Edge)> = edges(&all);
    let from_batch: Vec<(i64, Edge)> = spans
        .iter()
        .flat_map(|&(start, end)| [(start, Edge::Start), (end, Edge::End)])
        .collect();
    assert_eq!(rebuilt, from_batch);
}

#[test]
fn a_sample_stops_holding_at_its_timeout_online() {
    let silent = vec![Processor::new(
        "busy",
        Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 10),
    )];
    let mut live = LiveProcessor::new(&silent, 0).expect("valid");
    live.on_point("count", TimePointEvent::new(7, 9_i64))
        .expect("on time");

    // The hold ends at 17 whether or not a sample follows, so the span closes
    // as soon as the frontier passes it, with no sample after 7.
    assert_eq!(
        edges(&live.advance("count", 12).expect("known")),
        [(7, Edge::Start)]
    );
    assert_eq!(
        edges(&live.advance("count", 30).expect("known")),
        [(17, Edge::End)]
    );
    assert_eq!(live.frontier("busy"), Some(30));
    assert!(live.finish(40).expect("done").is_empty());
}

#[test]
fn a_stream_of_any_length_with_a_timeout_is_held_in_constant_space() {
    let silent = vec![
        Processor::new(
            "busy",
            Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 3),
        ),
        Processor::new("steps", Events::changes("count")),
    ];
    let mut live = LiveProcessor::new(&silent, 0).expect("valid");

    let mut most = 0;
    for step in 0..400_i64 {
        live.on_point("count", TimePointEvent::new(step * 10, 6 + step % 2))
            .expect("on time");
        live.advance("count", step * 10 + 5).expect("known");
        most = most.max(live.retained());
    }
    assert!(most <= 4, "retained {most}");
}

#[test]
fn changes_emit_each_point_once_its_predecessor_is_known() {
    let steps = vec![Processor::new("steps", Events::changes("count"))];
    let mut live = LiveProcessor::new(&steps, 0).expect("valid");
    let values = |emitted: &[Emission]| -> Vec<(i64, i64)> {
        emitted
            .iter()
            .map(|emission| match &emission.item {
                Emitted::Point(point) => (
                    point.timestamp(),
                    *point.value().downcast_ref::<i64>().expect("integers"),
                ),
                Emitted::Edge(_) => panic!("a point was expected"),
            })
            .collect()
    };

    // Nothing has a type before the first event, and the first one gives it.
    live.on_point("count", TimePointEvent::new(2, 4_i64))
        .expect("on time");
    live.on_point("count", TimePointEvent::new(5, 4_i64))
        .expect("on time");
    assert_eq!(values(&live.advance("count", 8).expect("known")), [(2, 4)]);

    // The sample before 12 is retained, so the repeat is still dropped.
    live.on_point("count", TimePointEvent::new(12, 4_i64))
        .expect("on time");
    live.on_point("count", TimePointEvent::new(14, 9_i64))
        .expect("on time");
    assert_eq!(
        values(&live.advance("count", 20).expect("known")),
        [(14, 9)]
    );

    assert!(matches!(
        live.on_point("count", TimePointEvent::new(21, "word")),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    assert!(live.finish(30).expect("done").is_empty());
}

#[test]
fn a_refused_first_event_does_not_give_an_input_its_type() {
    let steps = vec![Processor::new("steps", Events::changes("count"))];
    let mut live = LiveProcessor::new(&steps, 10).expect("valid");

    // Late for the frontier, so nothing is recorded and the type stays open.
    assert!(matches!(
        live.on_point("count", TimePointEvent::new(3, "word")),
        Err(ProcessError::Late { .. })
    ));
    live.on_point("count", TimePointEvent::new(12, 1_i64))
        .expect("the type is still open");
    assert_eq!(live.advance("count", 20).expect("known").len(), 1);
}

#[test]
fn a_read_of_a_changes_output_types_its_input() {
    let processor = vec![
        Processor::new("steps", Events::changes("count")),
        Processor::new(
            "busy",
            Formula::while_matching(Pattern::new("steps", Condition::Greater(5_i64))),
        ),
    ];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");

    assert!(matches!(
        live.on_point("count", TimePointEvent::new(1, "word")),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    live.on_point("count", TimePointEvent::new(1, 9_i64))
        .expect("on time");
    let emitted = live.advance("count", 5).expect("known");
    let busy: Vec<Emission> = emitted
        .into_iter()
        .filter(|emission| emission.key.as_str() == "busy")
        .collect();
    assert_eq!(edges(&busy), [(1, Edge::Start)]);
}
