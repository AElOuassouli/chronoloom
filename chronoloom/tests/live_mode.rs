//! A stream system in live mode: frontiers move with watermarks, and the
//! processors run incrementally over what became final. Whatever the way the
//! data is cut into updates and the watermarks that come with them, the derived
//! streams equal the batch result below their frontiers.

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;
#[path = "common/update.rs"]
mod update;

use std::collections::BTreeMap;

use chronoloom::processor::Processor;
use chronoloom::stream_system::{Live, StreamSystem, Update, UpdateError};
use chronoloom::{
    Condition, Edge, Events, Formula, Pattern, ProcessError, RecordError, Sequence,
    TimeIntervalSequence, TimePointEvent, TimePointSequence, Timestamp, Value,
};

use common::{bounds, for_each_seed, span, Random};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

const CASES: u64 = 300;

/// A live system holding the streams the random processors read, empty, from
/// the start of the stream.
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

/// The batch system over everything the live system holds of its streams, with
/// the same processors.
fn batch_of(live: &StreamSystem<Live>, processors: &[Processor]) -> StreamSystem {
    let mut streams = StreamSystem::new();
    for key in live.streams() {
        streams
            .insert_stream(
                key.clone(),
                live.get(key.as_str()).expect("a stream").clone(),
            )
            .expect("a stream of a batch system");
    }
    update::run(&streams, processors).expect("the batch result")
}

/// Every derived stream of `live` is the batch one below its frontier.
fn assert_equals_batch_below_frontiers(
    live: &StreamSystem<Live>,
    batch: &StreamSystem,
    context: &str,
) {
    for key in live.derived() {
        let frontier = live.frontier(key.as_str()).expect("a derived stream");
        let held = live.get(key.as_str()).expect("a derived stream");
        let expected = batch
            .get(key.as_str())
            .unwrap_or_else(|| panic!("{key} is not in the batch result: {context}"))
            .slice(..frontier);

        match (held, &expected) {
            (Sequence::TimeInterval(held), Sequence::TimeInterval(expected)) => {
                assert_eq!(
                    bounds(held),
                    bounds(expected),
                    "{key} below {frontier}: {context}"
                );
                assert_eq!(
                    held.observed(),
                    expected.observed(),
                    "{key} observed below {frontier}: {context}"
                );
            }
            (held, expected) => {
                assert_eq!(held, expected, "{key} below {frontier}: {context}");
            }
        }
    }
}

#[test]
fn live_streams_equal_batch_below_every_frontier_whatever_the_updates() {
    for_each_seed(CASES, |random, _| {
        let processors = processors(random);
        let stream: Stream = stream(random);
        let context = format!(
            "{} from {} over {:?}",
            processors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
            stream.from,
            stream.ops
        );

        let mut live = live_streams(stream.from);
        for processor in &processors {
            live.add_processor(processor.clone())
                .unwrap_or_else(|error| panic!("{error}: {context}"));
        }

        // Cut the operations into updates at random.
        let mut updates = Vec::new();
        let mut pending = Update::new();
        for operation in &stream.ops {
            push(&mut pending, operation);
            if random.chance(1, 3) {
                updates.push(std::mem::take(&mut pending));
            }
        }
        updates.push(pending);
        let mut horizon = Update::new();
        horizon.advance_all(stream.horizon);
        updates.push(horizon);

        // Adding a processor ran it over what was final already.
        let mut before: BTreeMap<String, Timestamp> = live
            .derived()
            .map(|key| {
                let frontier = live.frontier(key.as_str()).expect("a derived stream");
                (key.as_str().to_owned(), frontier)
            })
            .collect();
        let mut steps = Vec::new();
        for update in updates {
            let report = live
                .update(update)
                .unwrap_or_else(|error| panic!("{error}: {context}"));

            for (key, chunk) in report.chunks() {
                let range = chunk.range();
                assert_eq!(
                    range.start,
                    before.get(key.as_str()).copied().unwrap_or(Timestamp::MIN),
                    "{key}: the chunk continues the stream: {context}"
                );
                assert_eq!(
                    Some(&range.end),
                    report.frontiers().get(key),
                    "{key}: the chunk ends at the frontier: {context}"
                );
            }
            for (key, frontier) in report.frontiers() {
                let earlier = before.insert(key.as_str().to_owned(), *frontier);
                assert!(
                    earlier.is_none_or(|earlier| earlier <= *frontier),
                    "{key}: the frontier went back: {context}"
                );
                assert_eq!(live.frontier(key.as_str()), Some(*frontier));
            }
            steps.push(live.clone());
        }

        // Everything recorded: the batch result over it is the oracle at every
        // step, since what is final does not depend on what comes after.
        let batch = batch_of(&live, &processors);
        for step in &steps {
            assert_equals_batch_below_frontiers(step, &batch, &context);
        }
    });
}

#[test]
fn the_cut_into_updates_does_not_change_the_result() {
    for_each_seed(100, |random, _| {
        let processors = processors(random);
        let stream = stream(random);

        let run = |random: &mut Random, cut: u64| {
            let mut live = live_streams(stream.from);
            for processor in &processors {
                live.add_processor(processor.clone()).expect("a processor");
            }
            let mut pending = Update::new();
            for operation in &stream.ops {
                push(&mut pending, operation);
                if random.chance(1, cut) {
                    live.update(std::mem::take(&mut pending))
                        .expect("an update");
                }
            }
            live.update(pending).expect("an update");
            live.advance_all(stream.horizon).expect("an advance");
            live
        };

        let one_by_one = run(random, 1);
        let rarely = run(random, 9);
        let keys: Vec<_> = one_by_one.derived().cloned().collect();
        assert_eq!(keys, rarely.derived().cloned().collect::<Vec<_>>());
        for key in keys {
            assert_eq!(
                one_by_one.frontier(key.as_str()),
                rarely.frontier(key.as_str())
            );
            assert_eq!(
                one_by_one.get(key.as_str()),
                rarely.get(key.as_str()),
                "{key}"
            );
        }
    });
}

/// Motion events, a door timeline and presence between a start and an end of
/// motion.
fn room() -> StreamSystem<Live> {
    let mut room = StreamSystem::live(0);
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.insert_stream("door", TimeIntervalSequence::from_spans("door", Vec::new()))
        .unwrap();
    room.add_processor(Processor::new(
        "presence",
        Formula::between(
            Pattern::new("motion", Condition::Equals("start")),
            Pattern::new("motion", Condition::Equals("end")),
        ),
    ))
    .unwrap();
    room
}

#[test]
fn a_report_gives_the_newly_final_chunk_and_the_frontiers() {
    let mut room = room();
    assert_eq!(room.frontier("motion"), Some(0));
    assert_eq!(room.frontier("presence"), Some(0));

    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
            ],
        )
        .advance("motion", 30);
    let report = room.update(update).unwrap();

    let chunk = &report.chunks()["presence"];
    assert_eq!(chunk.range(), 0..30);
    let presence = chunk.sequence().as_time_interval().unwrap();
    assert_eq!(bounds(presence), [(10, 25)]);
    assert_eq!(
        chunk.edges().unwrap(),
        [
            TimePointEvent::new(10, Edge::Start),
            TimePointEvent::new(25, Edge::End)
        ]
    );
    assert_eq!(report.frontiers()["presence"], 30);
    assert_eq!(room.intervals("presence").map(bounds), Some(vec![(10, 25)]));

    // A later chunk continues the earlier one, and joins it in the system.
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(40, "start"),
                TimePointEvent::new(45, "end"),
            ],
        )
        .advance("motion", 50);
    let report = room.update(update).unwrap();
    assert_eq!(report.chunks()["presence"].range(), 30..50);
    assert_eq!(
        room.intervals("presence").map(bounds),
        Some(vec![(10, 25), (40, 45)])
    );

    // An advance that moves nothing makes no progress.
    let report = room.advance("motion", 20).unwrap();
    assert!(report.is_empty());
    assert_eq!(report.frontiers()["presence"], 50);
}

#[test]
fn a_span_open_at_the_frontier_continues_in_the_next_chunk() {
    let mut room = room();
    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(10, "start")])
        .advance("motion", 20);
    let report = room.update(update).unwrap();

    // The end is not known yet: the span is final as far as the frontier.
    assert_eq!(report.frontiers()["presence"], 20);
    assert_eq!(room.intervals("presence").map(bounds), Some(vec![(10, 20)]));

    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(32, "end")])
        .advance("motion", 40);
    room.update(update).unwrap();
    assert_eq!(room.intervals("presence").map(bounds), Some(vec![(10, 32)]));
}

#[test]
fn processors_read_the_outputs_of_those_before_them_in_order() {
    let mut room = room();
    room.add_processor(Processor::new(
        "occupied",
        Formula::state("presence").or(Formula::state("door")),
    ))
    .unwrap();

    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
            ],
        )
        .spans("door", [span(40, 50)])
        .advance_all(60);
    let report = room.update(update).unwrap();

    assert_eq!(report.frontiers()["occupied"], 60);
    assert_eq!(
        room.intervals("occupied").map(bounds),
        Some(vec![(10, 25), (40, 50)])
    );
    assert!(report.chunks().contains_key("occupied"));
}

#[test]
fn a_late_event_is_refused_and_nothing_is_recorded() {
    let mut room = room();
    room.advance("motion", 10).unwrap();
    let before = room.clone();

    let mut update = Update::new();
    update
        .spans("door", [span(2, 4)])
        .points(
            "motion",
            [
                TimePointEvent::new(12, "start"),
                TimePointEvent::new(9, "end"),
            ],
        )
        .advance("door", 30);
    let error = room.update(update).unwrap_err();

    assert_eq!(
        error,
        UpdateError::Late {
            key: "motion".into(),
            at: 9,
            frontier: 10,
        }
    );
    assert_eq!(room, before);
    assert_eq!(room.get("motion"), before.get("motion"));
    assert_eq!(room.get("door"), before.get("door"));
    assert_eq!(room.frontier("door"), Some(0));

    // At the frontier is not late.
    let mut update = Update::new();
    update.points("motion", [TimePointEvent::new(10, "start")]);
    room.update(update).unwrap();
    assert_eq!(
        room.points::<&str>("motion").map(TimePointSequence::len),
        Some(1)
    );

    // A span is late by where it starts.
    let mut update = Update::new();
    update.spans("door", [span(0, 30)]);
    assert!(room.update(update).is_ok());
    room.advance("door", 20).unwrap();
    let mut update = Update::new();
    update.spans("door", [span(19, 40)]);
    assert!(matches!(
        room.update(update),
        Err(UpdateError::Late {
            at: 19,
            frontier: 20,
            ..
        })
    ));
}

#[test]
fn watermarks_are_refused_for_unknown_and_derived_streams_and_in_batch() {
    let mut room = room();
    assert!(matches!(
        room.advance("nothing", 5),
        Err(UpdateError::Record(RecordError::Unknown { .. }))
    ));
    assert_eq!(
        room.advance("presence", 5).unwrap_err(),
        UpdateError::Derived {
            key: "presence".into()
        }
    );
    let mut recording = Update::new();
    recording.spans("presence", [span(0, 1)]);
    assert!(matches!(
        room.update(recording),
        Err(UpdateError::Derived { .. })
    ));

    let mut batch = StreamSystem::new();
    batch
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(1, "start")])
        .advance("motion", 5);
    assert_eq!(batch.update(update).unwrap_err(), UpdateError::Watermark);
    assert_eq!(
        batch.points::<&str>("motion").map(TimePointSequence::len),
        Some(0)
    );
}

#[test]
fn advancing_every_stream_moves_all_frontiers_and_never_back() {
    let mut room = room();
    room.advance("motion", 30).unwrap();
    room.advance_all(20).unwrap();

    assert_eq!(room.frontier("motion"), Some(30));
    assert_eq!(room.frontier("door"), Some(20));
    // Presence reads motion only.
    assert_eq!(room.frontier("presence"), Some(30));
    assert_eq!(room.frontier("nothing"), None);
}

#[test]
fn a_processor_added_late_runs_over_what_is_final() {
    let mut room = StreamSystem::live(0);
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
            ],
        )
        .advance_all(30);
    room.update(update).unwrap();

    let report = room
        .add_processor(Processor::new(
            "presence",
            Formula::between(
                Pattern::new("motion", Condition::Equals("start")),
                Pattern::new("motion", Condition::Equals("end")),
            ),
        ))
        .unwrap();
    assert_eq!(report.frontiers()["presence"], 30);
    assert_eq!(room.intervals("presence").map(bounds), Some(vec![(10, 25)]));
}

#[test]
fn a_processor_that_cannot_run_is_refused_and_a_transform_is_not_run_live() {
    let mut room = room();
    let before = room.clone();

    let unknown = room
        .add_processor(Processor::new("lost", Formula::state("nothing")))
        .unwrap_err();
    assert!(matches!(
        unknown,
        UpdateError::Process {
            error: ProcessError::UnknownKey { .. },
            ..
        }
    ));
    let clash = room
        .add_processor(Processor::new("door", Formula::state("motion")))
        .unwrap_err();
    assert!(matches!(clash, UpdateError::Taken { .. }));
    let transform = room
        .add_processor(Processor::new("motion", Events::changes("motion")))
        .unwrap_err();
    assert!(matches!(transform, UpdateError::LiveTransform { .. }));
    assert_eq!(room, before);
    assert_eq!(room.processors().len(), 1);
}

#[test]
fn a_stream_is_inserted_once_and_not_before_the_start() {
    let mut room = StreamSystem::live(10);
    room.insert_stream(
        "up",
        TimeIntervalSequence::from_spans("up", vec![span(12, 14)]),
    )
    .unwrap();

    assert!(matches!(
        room.insert_stream("up", TimeIntervalSequence::from_spans("up", Vec::new())),
        Err(UpdateError::Taken { .. })
    ));
    assert_eq!(
        room.insert_stream(
            "early",
            TimeIntervalSequence::from_spans("early", vec![span(3, 5)])
        )
        .unwrap_err(),
        UpdateError::Late {
            key: "early".into(),
            at: 3,
            frontier: 10,
        }
    );
    assert!(!room.contains("early"));
    assert_eq!(room.frontier("up"), Some(10));
}

#[test]
fn per_entity_members_appear_with_their_first_record() {
    let mut system = StreamSystem::live(0);
    system
        .insert_stream("records", TimePointSequence::<Value>::new("records"))
        .unwrap();
    system
        .add_processor(Processor::new(
            "seen",
            chronoloom::Recipe::per_entity(
                chronoloom::Group::new("records", "id"),
                Formula::while_matching(Pattern::field(
                    "records",
                    "ok",
                    Condition::Equals(Value::from(true)),
                )),
            ),
        ))
        .unwrap();

    let record =
        |id: i64, ok: bool| Value::record(vec![("id", Value::from(id)), ("ok", Value::from(ok))]);
    let mut update = Update::new();
    update
        .points(
            "records",
            [
                TimePointEvent::new(5, record(1, true)),
                TimePointEvent::new(8, record(2, false)),
            ],
        )
        .advance("records", 12);
    let report = system.update(update).unwrap();

    assert!(report.chunks().contains_key("seen/1"));
    assert!(system.contains("seen/2"));
    assert!(system.entity("seen/1").is_some());
    assert_eq!(report.frontiers().len(), system.derived().count());
}

#[test]
fn an_update_that_fails_after_recording_changes_nothing() {
    let mut room = room();
    let before = room.clone();

    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(5, "start")])
        .advance("motion", 20)
        .advance("nothing", 30);
    assert!(matches!(
        room.update(update),
        Err(UpdateError::Record(RecordError::Unknown { .. }))
    ));
    assert_eq!(room, before);
    assert_eq!(room.frontier("motion"), Some(0));
}

#[test]
fn a_watermark_may_pass_a_point_of_the_same_update_but_not_the_next_one() {
    let mut room = room();
    let mut update = Update::new();
    update
        .points("motion", [TimePointEvent::new(5, "start")])
        .advance("motion", 6);
    assert!(room.update(update).is_ok());

    let mut update = Update::new();
    update.points("motion", [TimePointEvent::new(5, "end")]);
    assert!(matches!(
        room.update(update),
        Err(UpdateError::Late {
            at: 5,
            frontier: 6,
            ..
        })
    ));
}

#[test]
fn a_sequence_system_refuses_an_update_with_watermarks() {
    let mut system = chronoloom::SequenceSystem::new();
    system.insert("p", TimePointSequence::<i64>::new("p"));
    let mut update = Update::new();
    update
        .points("p", [TimePointEvent::new(1, 1_i64)])
        .advance("p", 5);

    assert_eq!(system.record_update(update), Err(RecordError::Watermark));
    assert_eq!(
        system.points::<i64>("p").map(TimePointSequence::len),
        Some(0)
    );
}

#[test]
fn a_derived_stream_keeps_its_metadata_and_stays_derived_as_chunks_are_appended() {
    let mut room = room();
    room.advance("motion", 5).unwrap();
    room.member_metadata_mut("presence")
        .unwrap()
        .insert("unit".into(), "seconds".into());

    for round in 0..20_i64 {
        let start = 10 + round * 10;
        let mut update = Update::new();
        update
            .points(
                "motion",
                [
                    TimePointEvent::new(start, "start"),
                    TimePointEvent::new(start + 4, "end"),
                ],
            )
            .advance("motion", start + 8);
        room.update(update).unwrap();

        assert_eq!(
            room.member_metadata("presence")
                .and_then(|metadata| metadata.get("unit"))
                .map(String::as_str),
            Some("seconds")
        );
        assert!(room.is_derived("presence"));
        let presence = room.intervals("presence").unwrap();
        assert_eq!(presence.len(), usize::try_from(round + 1).unwrap());
        assert_eq!(presence.observed().as_slice().len(), 1);
    }
    // Writing into it is still refused.
    let mut recording = Update::new();
    recording.spans("presence", [span(500, 501)]);
    assert!(matches!(
        room.update(recording),
        Err(UpdateError::Derived { .. })
    ));
}
