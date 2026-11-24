//! The end of a live system's life and its changes of mode: `drain` hands the
//! final part over and keeps memory bounded, `finish` closes the streams, and
//! `into_live` starts a batch system's processors over the data held.

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;

use std::collections::BTreeMap;

use chronoloom::processor::Processor;
use chronoloom::stream_system::{Live, StreamSystem, Update, UpdateError};
use chronoloom::Events;
use chronoloom::{
    AnyValue, Condition, Edge, Formula, Group, Pattern, ProcessError, Recipe, RecordError, Schema,
    Sequence, TimeIntervalSequence, TimePointEvent, TimePointSequence, Timestamp, Value,
};

use common::{bounds, for_each_seed, property_cases, span, Random};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

/// A live system holding the streams the random processors read, empty.
fn live_streams(from: Timestamp) -> StreamSystem<Live> {
    let mut system = StreamSystem::live(from);
    for key in POINT_INPUTS {
        system
            .insert_stream(key, TimePointSequence::<i64>::new(key))
            .expect("a new stream");
    }
    for key in RECORD_INPUTS {
        system
            .insert_stream(key, TimePointSequence::<Value>::new(key))
            .expect("a new stream");
    }
    for key in STATE_INPUTS {
        system
            .insert_stream(key, TimeIntervalSequence::from_spans(key, Vec::new()))
            .expect("a new stream");
    }
    system
}

/// The operation as an update that carries only it.
fn push(update: &mut Update, operation: &Op) {
    match operation {
        Op::Point(key, at, value) => {
            update.points(*key, [TimePointEvent::new(*at, *value)]);
        }
        Op::Record(key, at, value) => {
            update.points(*key, [TimePointEvent::new(*at, value.clone())]);
        }
        Op::Span(key, start, end) => {
            update.spans(*key, [span(*start, *end)]);
        }
        Op::Advance(key, to) => {
            update.advance(*key, *to);
        }
    }
}

/// The operations of `stream`, cut at random into the groups that make an
/// update each.
fn groups(random: &mut Random, stream: &Stream) -> Vec<Vec<Op>> {
    let mut groups = Vec::new();
    let mut pending = Vec::new();
    for operation in &stream.ops {
        pending.push(operation.clone());
        if random.chance(1, 3) {
            groups.push(std::mem::take(&mut pending));
        }
    }
    groups.push(pending);
    groups
}

/// The update that carries the operations of a group.
fn build(group: &[Op]) -> Update {
    let mut update = Update::new();
    for operation in group {
        push(&mut update, operation);
    }
    update
}

/// One update of a run.
enum Step {
    Ops(Vec<Op>),
    Advance(Timestamp),
}

impl Step {
    /// The update the step makes; the same every time.
    fn update(&self) -> Update {
        match self {
            Self::Ops(group) => build(group),
            Self::Advance(watermark) => {
                let mut update = Update::new();
                update.advance_all(*watermark);
                update
            }
        }
    }
}

/// What a sequence holds, as comparable text: the spans of a timeline, and the
/// events of points with their values.
fn content(sequence: &Sequence) -> Vec<(Timestamp, String)> {
    match sequence {
        Sequence::TimeInterval(timeline) => bounds(timeline)
            .into_iter()
            .map(|(start, end)| (start, end.to_string()))
            .collect(),
        Sequence::TimePoint(_) => {
            fn events<T: std::fmt::Debug + 'static>(
                sequence: &Sequence,
            ) -> Option<Vec<(Timestamp, String)>> {
                let Sequence::TimePoint(points) = sequence else {
                    return None;
                };
                Some(
                    points
                        .downcast_ref::<T>()?
                        .iter()
                        .map(|event| (event.timestamp(), format!("{:?}", event.value())))
                        .collect(),
                )
            }
            events::<i64>(sequence)
                .or_else(|| events::<f64>(sequence))
                .or_else(|| events::<bool>(sequence))
                .or_else(|| events::<&str>(sequence))
                .or_else(|| events::<Value>(sequence))
                .or_else(|| events::<Edge>(sequence))
                .or_else(|| events::<AnyValue>(sequence))
                .expect("a value type the tests know")
        }
    }
}

/// The parts of one key joined end to end, as comparable content: touching
/// spans merge, as they do in a timeline.
fn joined(parts: &[&Sequence]) -> Vec<(Timestamp, String)> {
    let mut all: Vec<(Timestamp, String)> = parts.iter().flat_map(|part| content(part)).collect();
    if parts
        .iter()
        .any(|part| matches!(part, Sequence::TimeInterval(_)))
    {
        all.sort_by_key(|span| span.0);
        let mut merged: Vec<(Timestamp, i64)> = Vec::new();
        for (start, end) in all {
            let end: i64 = end.parse().expect("an end");
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        return merged
            .into_iter()
            .map(|(start, end)| (start, end.to_string()))
            .collect();
    }
    all
}

#[test]
fn drained_parts_joined_with_the_remainder_equal_the_system_that_never_drained() {
    for_each_seed(property_cases(), |random, _| {
        let processors = processors(random);
        let stream: Stream = stream(random);
        let context = format!("from {} over {:?}", stream.from, stream.ops);

        let mut draining = live_streams(stream.from);
        let mut reference = live_streams(stream.from);
        for processor in &processors {
            draining.add_processor(processor.clone()).expect("valid");
            reference.add_processor(processor.clone()).expect("valid");
        }

        let mut parts = Vec::new();
        let mut steps: Vec<Step> = groups(random, &stream).into_iter().map(Step::Ops).collect();
        steps.push(Step::Advance(stream.horizon));
        steps.push(Step::Advance(stream.horizon + 5));
        for step in &steps {
            let make = || step.update();
            let (mine, theirs) = (
                draining.update(make()).expect("an update"),
                reference.update(make()).expect("an update"),
            );
            // The processors never need what was drained.
            assert_eq!(mine, theirs, "{context}");
            if random.chance(1, 3) {
                parts.push(draining.drain());
            }
        }

        let keys: Vec<_> = reference.keys().cloned().collect();
        assert_eq!(
            keys,
            draining.keys().cloned().collect::<Vec<_>>(),
            "{context}"
        );
        for key in keys {
            let mut all: Vec<&Sequence> = parts
                .iter()
                .filter_map(|part| part.get(key.as_str()))
                .collect();
            all.push(draining.get(key.as_str()).expect("kept"));
            assert_eq!(
                joined(&all),
                content(reference.get(key.as_str()).expect("held")),
                "{key}: {context}"
            );
        }
        for part in &parts {
            assert_eq!(part.processors(), reference.processors());
        }
    });
}

#[test]
fn a_drained_system_keeps_what_the_processors_made_of_the_data() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    live.member_metadata_mut("motion")
        .unwrap()
        .insert("unit".to_owned(), "meters".to_owned());
    live.add_processor(Processor::new(
        "presence",
        Formula::between(
            Pattern::new("motion", Condition::Equals("start")),
            Pattern::new("motion", Condition::Equals("end")),
        ),
    ))
    .unwrap();

    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
                TimePointEvent::new(40, "start"),
            ],
        )
        .advance("motion", 30);
    live.update(update).unwrap();

    let drained = live.drain();
    assert_eq!(drained.processors(), live.processors());
    assert_eq!(
        drained.intervals("presence").map(bounds),
        Some(vec![(10, 25)])
    );
    assert!(drained.is_derived("presence"));
    assert_eq!(
        drained
            .member_metadata("motion")
            .and_then(|metadata| metadata.get("unit")),
        Some(&"meters".to_owned())
    );
    assert_eq!(
        drained.points::<&str>("motion").map(|points| points.len()),
        Some(2)
    );

    // The live system keeps what is not final, and goes on.
    assert_eq!(
        live.points::<&str>("motion").map(|points| points.len()),
        Some(1)
    );
    assert_eq!(live.intervals("presence").map(bounds), Some(Vec::new()));
    assert!(live.is_derived("presence"));
    assert_eq!(live.frontier("presence"), Some(30));
    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(45, "end")])
        .advance("motion", 50);
    let report = live.update(update).unwrap();
    assert_eq!(report.chunks()["presence"].range(), 30..50);
    assert_eq!(live.intervals("presence").map(bounds), Some(vec![(40, 45)]));
}

#[test]
fn a_processor_added_after_a_drain_starts_over_what_is_still_held() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
                TimePointEvent::new(40, "start"),
            ],
        )
        .advance("motion", 30);
    live.update(update).unwrap();
    drop(live.drain());

    live.add_processor(Processor::new(
        "presence",
        Formula::between(
            Pattern::new("motion", Condition::Equals("start")),
            Pattern::new("motion", Condition::Equals("end")),
        ),
    ))
    .unwrap();
    // The first span was drained: only the data from 40 on is read.
    assert_eq!(live.intervals("presence").map(bounds), Some(Vec::new()));
    assert_eq!(live.frontier("presence"), Some(30));
}

/// The number of events and spans a live system holds.
fn held(system: &StreamSystem<Live>) -> usize {
    system
        .keys()
        .map(|key| system.get(key.as_str()).expect("held").len())
        .sum()
}

fn gate(agent: i64, direction: &str) -> Value {
    Value::record([
        ("agent", Value::from(agent)),
        ("direction", Value::from(direction)),
    ])
}

fn presence_per_agent() -> Recipe {
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    Recipe::per_entity(
        Group::new("gate", "agent"),
        Formula::between(direction("in"), direction("out")),
    )
}

/// The most a live system holds when it is drained after every tick of a stream
/// of `ticks` instants.
fn worst_held(ticks: i64) -> usize {
    let mut live = StreamSystem::live(0);
    live.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    live.insert_stream("door", TimeIntervalSequence::from_spans("door", Vec::new()))
        .unwrap();
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    live.add_processor(Processor::new(
        "presence",
        Formula::between(
            Pattern::new("motion", Condition::Equals("start")),
            Pattern::new("motion", Condition::Equals("end")),
        ),
    ))
    .unwrap();
    live.add_processor(Processor::new(
        "occupied",
        Formula::state("presence").or(Formula::state("door")),
    ))
    .unwrap();
    live.add_processor(Processor::new("agents", presence_per_agent()))
        .unwrap();
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .unwrap();
    live.add_processor(Processor::new(
        "high",
        Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 3),
    ))
    .unwrap();
    live.add_processor(Processor::new("steps", Events::changes("count")))
        .unwrap();
    live.add_processor(Processor::new(
        "early",
        Formula::state("presence").shift(-2, 2),
    ))
    .unwrap();

    let mut worst = 0;
    for tick in 0..ticks {
        let mut update = Update::new();
        update.points(
            "motion",
            [TimePointEvent::new(
                tick,
                if tick % 2 == 0 { "start" } else { "end" },
            )],
        );
        if tick % 4 == 0 {
            update.spans("door", [span(tick, tick + 1)]);
        }
        let way = if (tick / 5) % 2 == 0 { "in" } else { "out" };
        update.points("gate", [TimePointEvent::new(tick, gate(tick % 5, way))]);
        update.points("count", [TimePointEvent::new(tick, 6 + tick % 2)]);
        update.advance_all(tick + 1);
        live.update(update).expect("an update");
        drop(live.drain());
        worst = worst.max(held(&live));
    }
    worst
}

#[test]
fn what_a_drained_live_system_holds_does_not_grow_with_the_stream() {
    let short = worst_held(200);
    let long = worst_held(2_000);

    assert_eq!(short, long, "ten times the stream holds the same");
    assert!(long < 40, "held {long}");
}

#[test]
fn finishing_closes_the_streams_and_flushes_the_open_spans() {
    let busy = Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64))),
    );
    let mut live = StreamSystem::live(0);
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .unwrap();
    live.add_processor(busy).unwrap();

    let mut update = Update::new();
    update
        .points("count", [TimePointEvent::new(7, 9_i64)])
        .advance("count", 10);
    live.update(update).unwrap();
    assert_eq!(live.intervals("busy").map(bounds), Some(vec![(7, 10)]));
    assert!(live
        .advance("count", 20)
        .unwrap()
        .chunks()
        .contains_key("busy"));

    let report = live.finish(30).unwrap();
    assert_eq!(live.frontier("busy"), Some(Timestamp::MAX));
    assert_eq!(report.frontiers()["busy"], Timestamp::MAX);
    assert_eq!(live.intervals("busy").map(bounds), Some(vec![(7, 30)]));

    // A second call changes nothing, and a finished system takes no event.
    let before = live.clone();
    assert!(live.finish(40).unwrap().is_empty());
    assert_eq!(live, before);
    let mut late = Update::new();
    late.points("count", [TimePointEvent::new(35, 9_i64)]);
    assert!(matches!(live.update(late), Err(UpdateError::Late { .. })));
    assert_eq!(live.intervals("busy").map(bounds), Some(vec![(7, 30)]));
}

#[test]
fn nothing_is_known_about_what_nothing_observed() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .unwrap();
    live.add_processor(Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64))),
    ))
    .unwrap();
    let mut update = Update::new();
    update
        .points("count", [TimePointEvent::new(4, 1_i64)])
        .advance("count", 50);
    live.update(update).unwrap();

    live.finish(60).unwrap();
    assert_eq!(live.intervals("busy").map(bounds), Some(Vec::new()));
}

#[test]
fn finishing_gives_the_batch_result_over_the_streams() {
    for_each_seed(property_cases(), |random, _| {
        let processors = processors(random);
        let stream: Stream = stream(random);
        let context = format!(
            "from {} until {} over {:?}",
            stream.from, stream.horizon, stream.ops
        );

        let mut live = live_streams(stream.from);
        for processor in &processors {
            live.add_processor(processor.clone()).expect("valid");
        }
        for group in groups(random, &stream) {
            live.update(build(&group)).expect("an update");
        }
        // Sometimes the streams are advanced to the horizon before it.
        if random.chance(1, 2) {
            live.update(Step::Advance(stream.horizon).update())
                .expect("an advance");
        }
        live.finish(stream.horizon).expect("finish");

        let mut batch = StreamSystem::new();
        for key in live.streams() {
            batch
                .insert_stream(
                    key.clone(),
                    live.get(key.as_str()).expect("a stream").clone(),
                )
                .expect("a stream");
        }
        for processor in &processors {
            batch.add_processor(processor.clone()).expect("valid");
        }
        for key in live.derived() {
            assert_eq!(
                content(live.get(key.as_str()).expect("held")),
                content(batch.get(key.as_str()).expect("derived in batch")),
                "{key}: {context}"
            );
        }
    });
}

/// The streams and processors of a room, at the start of a live system or of a
/// batch one.
fn room_processors() -> Vec<Processor> {
    vec![
        Processor::new(
            "presence",
            Formula::between(
                Pattern::new("motion", Condition::Equals("start")),
                Pattern::new("motion", Condition::Equals("end")),
            ),
        ),
        Processor::new(
            "occupied",
            Formula::state("presence").or(Formula::state("door")),
        ),
        Processor::new("agents", presence_per_agent()),
    ]
}

fn first_updates() -> Update {
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(3, "start"),
                TimePointEvent::new(8, "end"),
                TimePointEvent::new(12, "start"),
            ],
        )
        .spans("door", [span(2, 6)])
        .points(
            "gate",
            [
                TimePointEvent::new(4, gate(1, "in")),
                TimePointEvent::new(9, gate(2, "in")),
                TimePointEvent::new(13, gate(1, "out")),
            ],
        );
    update
}

fn later_updates() -> Update {
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(20, "end"),
                TimePointEvent::new(25, "start"),
                TimePointEvent::new(30, "end"),
            ],
        )
        .spans("door", [span(18, 22)])
        .points(
            "gate",
            [
                TimePointEvent::new(21, gate(2, "out")),
                TimePointEvent::new(26, gate(3, "in")),
            ],
        )
        .advance_all(40);
    update
}

fn room_streams() -> Vec<(&'static str, Sequence)> {
    vec![
        ("motion", TimePointSequence::<&str>::new("motion").into()),
        (
            "door",
            TimeIntervalSequence::from_spans("door", Vec::new()).into(),
        ),
        ("gate", TimePointSequence::<Value>::new("gate").into()),
    ]
}

#[test]
fn into_live_then_updates_equals_live_from_the_start() {
    let cut = 15;

    let mut batch = StreamSystem::new();
    for (key, sequence) in room_streams() {
        batch.insert_stream(key, sequence).unwrap();
    }
    batch.update(first_updates()).unwrap();
    // What the batch system holds beyond the cut is not carried over.
    let mut beyond = Update::new();
    beyond.points("motion", [TimePointEvent::new(50, "start")]);
    batch.update(beyond).unwrap();
    for processor in room_processors() {
        batch.add_processor(processor).unwrap();
    }
    let mut converted = batch.into_live(cut).unwrap();

    let mut started = StreamSystem::live(0);
    for (key, sequence) in room_streams() {
        started.insert_stream(key, sequence).unwrap();
    }
    for processor in room_processors() {
        started.add_processor(processor).unwrap();
    }
    started.update(first_updates()).unwrap();
    started.advance_all(cut).unwrap();

    for key in started.keys() {
        assert_eq!(
            converted.frontier(key.as_str()),
            started.frontier(key.as_str())
        );
        assert_eq!(
            content(converted.get(key.as_str()).expect("held")),
            content(started.get(key.as_str()).expect("held")),
            "{key} at the cut"
        );
    }

    let ours = converted.update(later_updates()).unwrap();
    let theirs = started.update(later_updates()).unwrap();
    assert_eq!(ours, theirs);
    for key in started.keys() {
        assert_eq!(
            content(converted.get(key.as_str()).expect("held")),
            content(started.get(key.as_str()).expect("held")),
            "{key}"
        );
    }
    assert_eq!(
        converted.intervals("presence").map(bounds),
        Some(vec![(3, 8), (12, 20), (25, 30)])
    );
}

#[test]
fn a_batch_system_that_transforms_cannot_go_live() {
    let mut batch = StreamSystem::new();
    batch
        .insert_stream("count", TimePointSequence::<i64>::new("count"))
        .unwrap();
    batch
        .add_processor(Processor::new(
            "count",
            chronoloom::Events::changes("count"),
        ))
        .unwrap();

    assert!(matches!(
        batch.into_live(0),
        Err(UpdateError::LiveTransform { .. })
    ));
}

#[test]
fn two_entities_with_one_name_are_refused_and_nothing_is_recorded() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    live.add_processor(Processor::new("presence", presence_per_agent()))
        .unwrap();
    let record = |agent: Value, direction: &str| {
        Value::record([("agent", agent), ("direction", Value::from(direction))])
    };

    let mut first = Update::new();
    first
        .points(
            "gate",
            [TimePointEvent::new(10, record(Value::from("5"), "in"))],
        )
        .advance("gate", 11);
    live.update(first).unwrap();
    let before = live.clone();

    let mut clash = Update::new();
    clash
        .points(
            "gate",
            [TimePointEvent::new(12, record(Value::from(5), "in"))],
        )
        .advance("gate", 13);
    assert!(matches!(
        live.update(clash),
        Err(UpdateError::Process {
            error: ProcessError::DuplicateMember { key },
            ..
        }) if key.as_str() == "presence/5"
    ));
    assert_eq!(live, before);
}

#[test]
fn an_output_cannot_take_the_key_of_an_entity() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    live.insert_stream("door", TimeIntervalSequence::from_spans("door", Vec::new()))
        .unwrap();
    live.add_processor(Processor::new("presence", presence_per_agent()))
        .unwrap();
    let clash = live.add_processor(Processor::new("presence/7", Formula::state("door")));

    assert!(matches!(clash, Err(UpdateError::Taken { key }) if key.as_str() == "presence/7"));
    assert_eq!(live.processors().len(), 1);
}

#[test]
fn a_map_of_frontiers_after_a_drain_is_unchanged() {
    let mut live = live_streams(0);
    live.add_processor(Processor::new("seen", Formula::state("s")))
        .unwrap();
    live.advance_all(25).unwrap();
    let before: BTreeMap<_, _> = live
        .keys()
        .map(|key| (key.clone(), live.frontier(key.as_str())))
        .collect();

    drop(live.drain());
    let after: BTreeMap<_, _> = live
        .keys()
        .map(|key| (key.clone(), live.frontier(key.as_str())))
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_shift_lags_its_input_by_how_far_it_moves_back() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("door", TimePointSequence::<i64>::new("door"))
        .unwrap();
    let shifted = Formula::occurs(Pattern::new("door", Condition::Any::<i64>));
    live.add_processor(Processor::new("early", shifted.clone().shift(-2, 2)))
        .unwrap();
    live.add_processor(Processor::new("late", shifted.shift(3, 5)))
        .unwrap();

    live.advance("door", 10).unwrap();
    // A shift back lags its input by the larger move back; one forward never gets
    // ahead of its input.
    assert_eq!(live.frontier("early"), Some(8));
    assert_eq!(live.frontier("late"), Some(10));
}

#[test]
fn chained_outputs_chain_their_lags() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("door", TimePointSequence::<i64>::new("door"))
        .unwrap();
    live.add_processor(Processor::new(
        "first",
        Formula::occurs(Pattern::new("door", Condition::Any::<i64>)).shift(-1, -1),
    ))
    .unwrap();
    live.add_processor(Processor::new(
        "second",
        Formula::state("first").shift(-2, -2),
    ))
    .unwrap();

    let mut update = Update::new();
    update
        .points("door", [TimePointEvent::new(5, 1_i64)])
        .advance("door", 10);
    live.update(update).unwrap();
    assert_eq!(live.frontier("second"), Some(7));
    // `second` is `[5, 6)` moved back three ticks.
    assert_eq!(live.intervals("second").map(bounds), Some(vec![(2, 3)]));
}

#[test]
fn head_is_the_frontier_online() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("motion", TimePointSequence::<i64>::new("motion"))
        .unwrap();
    live.add_processor(Processor::new(
        "open",
        Formula::between_with(
            Pattern::new("motion", Condition::Equals(1_i64)),
            Pattern::new("motion", Condition::Equals(0_i64)),
            chronoloom::OpenEnd::Head,
        ),
    ))
    .unwrap();

    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(5, 1_i64)])
        .advance("motion", 20);
    live.update(update).unwrap();
    // The head is the latest point plus one, so nothing past it is final yet.
    assert_eq!(live.frontier("open"), Some(6));
    live.finish(25).unwrap();
    assert_eq!(live.intervals("open").map(bounds), Some(vec![(5, 6)]));
    assert_eq!(live.frontier("open"), Some(Timestamp::MAX));
}

#[test]
fn another_output_reads_the_union_and_the_count_online() {
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    let present = || Formula::between(direction("in"), direction("out"));
    let by_agent = || Group::new("gate", "agent");

    let mut live = StreamSystem::live(0);
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    live.add_processor(Processor::new(
        "anyone",
        Recipe::any_entity(by_agent(), present()),
    ))
    .unwrap();
    live.add_processor(Processor::new(
        "crowd",
        Recipe::count_entities(by_agent(), present()),
    ))
    .unwrap();
    live.add_processor(Processor::new(
        "alone",
        Formula::while_matching(Pattern::new("crowd", Condition::Equals(1_i64)))
            .and(Formula::state("anyone")),
    ))
    .unwrap();

    let mut update = Update::new();
    update
        .points(
            "gate",
            [
                TimePointEvent::new(10, gate(7, "in")),
                TimePointEvent::new(12, gate(9, "in")),
                TimePointEvent::new(20, gate(7, "out")),
            ],
        )
        .advance("gate", 25);
    live.update(update).unwrap();
    live.finish(40).unwrap();

    // Alone from 10 to 12, and again from 20 to the end.
    assert_eq!(
        live.intervals("alone").map(bounds),
        Some(vec![(10, 12), (20, 40)])
    );
}

#[test]
fn into_live_keeps_the_metadata_of_streams_and_derived_streams() {
    let mut batch = StreamSystem::new();
    batch
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    batch
        .add_processor(Processor::new(
            "presence",
            Formula::between(
                Pattern::new("motion", Condition::Equals("start")),
                Pattern::new("motion", Condition::Equals("end")),
            ),
        ))
        .unwrap();
    batch
        .member_metadata_mut("motion")
        .unwrap()
        .insert("unit".to_owned(), "meters".to_owned());
    batch
        .member_metadata_mut("presence")
        .unwrap()
        .insert("owner".to_owned(), "lobby".to_owned());

    let live = batch.into_live(10).unwrap();
    assert_eq!(
        live.member_metadata("motion").and_then(|m| m.get("unit")),
        Some(&"meters".to_owned())
    );
    assert_eq!(
        live.member_metadata("presence")
            .and_then(|m| m.get("owner")),
        Some(&"lobby".to_owned())
    );
    assert!(live.is_derived("presence"));
}

/// A live system with one stream of records `{ n: Integer }`, schema set
/// before going live, starting at 0.
fn schema_live() -> StreamSystem<Live> {
    let mut batch = StreamSystem::new();
    batch
        .insert_stream("records", TimePointSequence::<Value>::new("records"))
        .unwrap();
    batch
        .set_schema("records", Schema::record([("n", Schema::Integer)]))
        .unwrap();
    batch.into_live(0).unwrap()
}

fn record(n: i64) -> Value {
    Value::record([("n", Value::from(n))])
}

fn wrong_record() -> Value {
    Value::record([("n", Value::from("text"))])
}

#[test]
fn a_live_stream_reports_lateness_before_the_schema() {
    let mut live = schema_live();
    let mut update = Update::new();
    update
        .points("records", [TimePointEvent::new(5, record(1))])
        .advance("records", 10);
    live.update(update).unwrap();

    // Late and also without the shape: lateness is reported first.
    let mut update = Update::new();
    update.points("records", [TimePointEvent::new(3, wrong_record())]);
    assert!(matches!(
        live.update(update),
        Err(UpdateError::Late { at: 3, .. })
    ));

    // On time and without the shape: the schema refuses it.
    let mut update = Update::new();
    update.points("records", [TimePointEvent::new(12, wrong_record())]);
    assert!(matches!(
        live.update(update),
        Err(UpdateError::Record(RecordError::SchemaMismatch { .. }))
    ));
    assert_eq!(
        live.points::<Value>("records").map(TimePointSequence::len),
        Some(1)
    );
}

#[test]
fn draining_keeps_the_schema_on_both_halves() {
    let mut live = schema_live();
    let mut update = Update::new();
    update
        .points(
            "records",
            [
                TimePointEvent::new(5, record(1)),
                TimePointEvent::new(15, record(2)),
            ],
        )
        .advance("records", 10);
    live.update(update).unwrap();

    let drained = live.drain();
    assert!(live.schema("records").is_some());
    assert_eq!(live.schema("records"), drained.schema("records"));
    assert_eq!(
        drained
            .points::<Value>("records")
            .map(TimePointSequence::len),
        Some(1)
    );
    assert_eq!(
        live.points::<Value>("records").map(TimePointSequence::len),
        Some(1)
    );

    // What stays still checks what arrives.
    let mut update = Update::new();
    update.points("records", [TimePointEvent::new(20, wrong_record())]);
    assert!(matches!(
        live.update(update),
        Err(UpdateError::Record(RecordError::SchemaMismatch { .. }))
    ));
}

#[test]
fn a_stream_with_nothing_final_is_drained_empty_with_its_extent() {
    let mut live = schema_live();
    let mut update = Update::new();
    update
        .points("records", [TimePointEvent::new(15, record(1))])
        .advance("records", 10);
    live.update(update).unwrap();

    let drained = live.drain();
    let part = drained
        .points::<Value>("records")
        .expect("present in the drained system");
    assert!(part.is_empty());
    assert!(part.is_observed(5) && !part.is_observed(12));
    assert_eq!(
        live.points::<Value>("records").map(TimePointSequence::len),
        Some(1)
    );
}
