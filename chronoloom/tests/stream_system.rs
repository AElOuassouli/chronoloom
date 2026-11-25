//! A stream system in batch mode: streams, a list of one-output processors,
//! and derived streams that follow every change.

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;
#[path = "common/update.rs"]
mod update;

use chronoloom::processor::Processor;
use chronoloom::stream_system::{StreamSystem, Update, UpdateError};
use chronoloom::{
    Condition, Events, Extent, Formula, Group, Pattern, ProcessError, Recipe, Schema,
    TimeIntervalSequence, TimePointEvent, TimePointSequence, Value,
};

use common::{bounds, for_each_seed, property_cases, span, Random};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

/// A room: motion events, a door timeline, presence derived from motion, and
/// occupancy derived from presence and the door.
fn room() -> StreamSystem {
    let mut room = StreamSystem::new();
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.insert_stream(
        "door",
        TimeIntervalSequence::from_spans("door", vec![span(40, 50)]),
    )
    .unwrap();
    room.add_processor(Processor::new("presence", presence()))
        .unwrap();
    room.add_processor(Processor::new(
        "occupied",
        Formula::state("presence").or(Formula::state("door")),
    ))
    .unwrap();
    room
}

fn presence() -> Formula {
    Formula::between(
        Pattern::new("motion", Condition::Equals("start")),
        Pattern::new("motion", Condition::Equals("end")),
    )
}

fn motion(events: &[(i64, &'static str)]) -> Update {
    let mut update = Update::new();
    update.points(
        "motion",
        events
            .iter()
            .map(|&(timestamp, word)| TimePointEvent::new(timestamp, word)),
    );
    update
}

#[test]
fn an_update_reaches_every_processor_that_reads_it_and_no_other() {
    let mut room = room();
    room.insert_stream("light", TimePointSequence::<f64>::new("light"))
        .unwrap();
    room.add_processor(Processor::new(
        "bright",
        Formula::while_matching(Pattern::new("light", Condition::Greater(100.0))),
    ))
    .unwrap();

    let report = room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    assert_eq!(
        report
            .changed()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["occupied", "presence"]
    );
    assert_eq!(bounds(room.intervals("presence").unwrap()), [(10, 25)]);
    assert_eq!(
        bounds(room.intervals("occupied").unwrap()),
        [(10, 25), (40, 50)]
    );

    // Light reaches only the processor that reads it.
    let mut light = Update::new();
    light.points("light", [TimePointEvent::new(5, 150.0_f64)]);
    let report = room.update(light).unwrap();
    assert_eq!(
        report
            .changed()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        ["bright"]
    );
}

#[test]
fn an_update_reaches_a_processor_through_two_levels_of_outputs() {
    let mut room = room();
    room.add_processor(Processor::new(
        "alarm",
        Formula::state("occupied").and(Formula::state("door")),
    ))
    .unwrap();
    room.insert_stream("light", TimePointSequence::<f64>::new("light"))
        .unwrap();
    room.add_processor(Processor::new(
        "bright",
        Formula::while_matching(Pattern::new("light", Condition::Greater(100.0))),
    ))
    .unwrap();

    // Motion reaches `alarm` only through `presence` and `occupied`.
    let report = room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    let evaluated: Vec<&str> = report.evaluated().iter().map(|key| key.as_str()).collect();
    assert_eq!(evaluated, ["alarm", "occupied", "presence"]);
    assert_eq!(bounds(room.intervals("alarm").unwrap()), [(40, 50)]);
}

#[test]
fn a_processor_that_reads_nothing_that_changed_is_not_evaluated() {
    let mut room = room();
    room.insert_stream("light", TimePointSequence::<f64>::new("light"))
        .unwrap();
    room.add_processor(Processor::new(
        "bright",
        Formula::while_matching(Pattern::new("light", Condition::Greater(100.0))),
    ))
    .unwrap();

    let report = room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    assert!(!report.evaluated().contains("bright"));
    assert!(report.evaluated().contains("presence"));
}

#[test]
fn a_per_entity_output_and_another_output_never_nest() {
    let per_entity = || {
        Processor::new(
            "presence",
            Recipe::per_entity(Group::new("gate", "agent"), Formula::state("gate")),
        )
    };
    let gate = || {
        let mut gate = StreamSystem::new();
        gate.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
            .unwrap();
        gate.insert_stream("door", TimeIntervalSequence::new("door"))
            .unwrap();
        gate
    };

    // An output under the per-entity output of another processor.
    let mut first = gate();
    first.add_processor(per_entity()).unwrap();
    assert_eq!(
        first.add_processor(Processor::new("presence/ada", Formula::state("door"))),
        Err(UpdateError::Taken {
            key: "presence/ada".into()
        })
    );

    // A per-entity output over an output, or a stream, already under it.
    let mut second = gate();
    second
        .add_processor(Processor::new("presence/ada", Formula::state("door")))
        .unwrap();
    assert_eq!(
        second.add_processor(per_entity()),
        Err(UpdateError::Taken {
            key: "presence".into()
        })
    );
    assert!(second.is_derived("presence/ada"));

    // A stream put under a per-entity output afterwards.
    assert!(matches!(
        first.insert_stream("presence/carol", TimeIntervalSequence::new("carol")),
        Err(UpdateError::Derived { .. })
    ));

    let mut third = gate();
    third
        .insert_stream("presence/bob", TimeIntervalSequence::new("presence/bob"))
        .unwrap();
    assert!(matches!(
        third.add_processor(per_entity()),
        Err(UpdateError::Taken { .. })
    ));
}

#[test]
fn streams_and_derived_streams_are_told_apart() {
    let room = room();
    let streams: Vec<&str> = room.streams().map(|key| key.as_str()).collect();
    let derived: Vec<&str> = room.derived().map(|key| key.as_str()).collect();
    assert_eq!(streams, ["door", "motion"]);
    assert_eq!(derived, ["occupied", "presence"]);
    assert!(room.is_derived("presence") && !room.is_derived("motion"));
    assert_eq!(
        room.producer("occupied")
            .map(|processor| processor.output().as_str()),
        Some("occupied")
    );
    let inputs: Vec<&str> = room.processors()[1]
        .inputs()
        .into_iter()
        .map(|key| key.as_str())
        .collect();
    assert_eq!(inputs, ["door", "presence"]);
}

#[test]
fn the_producer_of_a_key_is_found_by_the_key_alone() {
    let mut room = room();
    let producer_of = |system: &StreamSystem, key: &str| {
        system
            .producer(key)
            .map(|processor| processor.output().as_str().to_owned())
    };

    // A slice and a clone keep who produces what, with no recipe stored on
    // the members.
    let slice = room.slice(0..5);
    assert_eq!(producer_of(&slice, "presence").as_deref(), Some("presence"));
    assert!(slice.is_derived("occupied") && !slice.is_derived("motion"));
    assert_eq!(
        producer_of(&room.clone(), "occupied").as_deref(),
        Some("occupied")
    );

    // What the processor writes follows the processor: removed, the key is
    // neither derived nor produced, and a stream can take it.
    room.remove_processor("occupied").unwrap();
    assert!(!room.contains("occupied"));
    room.insert_stream("occupied", TimeIntervalSequence::new("occupied"))
        .unwrap();
    assert!(!room.is_derived("occupied") && room.producer("occupied").is_none());
}

#[test]
fn a_derived_stream_is_read_only() {
    let mut room = room();
    let before = room.clone();

    let mut into_derived = Update::new();
    into_derived.spans("presence", [span(0, 5)]);
    assert_eq!(
        room.update(into_derived),
        Err(UpdateError::Derived {
            key: "presence".into()
        })
    );
    assert!(matches!(
        room.insert_stream("occupied", TimeIntervalSequence::new("x")),
        Err(UpdateError::Derived { .. })
    ));
    assert!(matches!(
        room.remove_stream("presence"),
        Err(UpdateError::Derived { .. })
    ));
    assert!(matches!(
        room.relabel("presence", "x"),
        Err(UpdateError::Derived { .. })
    ));
    assert_eq!(room.schema("presence"), None);
    assert!(matches!(
        room.set_schema("presence", Schema::Integer),
        Err(UpdateError::Derived { .. })
    ));
    assert_eq!(room, before);
}

#[test]
fn a_processor_reads_only_what_exists_and_takes_a_free_key() {
    let mut room = room();
    let before = room.clone();

    // Its own output, or a key the system does not hold.
    assert!(matches!(
        room.add_processor(Processor::new("loop", Formula::state("loop"))),
        Err(UpdateError::Process {
            error: ProcessError::UnknownKey { .. },
            ..
        })
    ));
    assert!(matches!(
        room.add_processor(Processor::new("later", Formula::state("missing"))),
        Err(UpdateError::Process { .. })
    ));
    // An output key already in use, by a stream or another processor.
    for (taken, read) in [("door", "presence"), ("presence", "door")] {
        assert_eq!(
            room.add_processor(Processor::new(taken, Formula::state(read))),
            Err(UpdateError::Taken { key: taken.into() })
        );
    }
    // A member of another shape than the formula reads.
    assert!(matches!(
        room.add_processor(Processor::new(
            "wrong",
            Formula::occurs(Pattern::new("door", Condition::<i64>::Any))
        )),
        Err(UpdateError::Process { .. })
    ));
    assert_eq!(room, before);
}

#[test]
fn what_a_processor_reads_cannot_be_removed() {
    let mut room = room();
    let before = room.clone();

    assert_eq!(
        room.remove_stream("motion"),
        Err(UpdateError::Read {
            key: "motion".into(),
            by: "presence".into()
        })
    );
    assert_eq!(
        room.remove_processor("presence"),
        Err(UpdateError::Read {
            key: "presence".into(),
            by: "occupied".into()
        })
    );
    assert_eq!(
        room.remove_processor("nothing"),
        Err(UpdateError::UnknownProcessor {
            key: "nothing".into()
        })
    );
    assert_eq!(room, before);

    // From the end of the list, everything comes apart.
    room.remove_processor("occupied").unwrap();
    room.remove_processor("presence").unwrap();
    assert_eq!(room.derived().count(), 0);
    assert!(room.remove_stream("motion").unwrap().is_some());
}

#[test]
fn a_change_a_processor_cannot_follow_changes_nothing() {
    let mut room = room();
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    let before = room.clone();

    // Motion as a timeline: presence reads points, so the change is refused.
    let replaced = room.insert_stream("motion", TimeIntervalSequence::new("motion"));
    assert!(matches!(
        replaced,
        Err(UpdateError::Process { processor, .. }) if processor.as_str() == "presence"
    ));
    // A point of another value type is refused by the recording itself.
    let mut wrong = Update::new();
    wrong.points("motion", [TimePointEvent::new(30, 1_i64)]);
    assert!(matches!(room.update(wrong), Err(UpdateError::Record(_))));
    assert_eq!(room, before);
}

#[test]
fn a_derived_stream_keeps_its_metadata_when_it_follows_a_change() {
    let mut room = room();
    room.member_metadata_mut("presence")
        .unwrap()
        .insert("unit".into(), "seconds".into());
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    assert_eq!(
        room.member_metadata("presence")
            .and_then(|metadata| metadata.get("unit"))
            .map(String::as_str),
        Some("seconds")
    );
}

#[test]
fn a_per_entity_processor_produces_a_family_that_follows_its_entities() {
    let mut gate = StreamSystem::new();
    gate.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    let pass = |agent: &str, direction: &str| {
        Value::record([
            ("agent", Value::from(agent)),
            ("direction", Value::from(direction)),
        ])
    };
    let inside = Formula::between(
        Pattern::field("gate", "direction", Condition::Equals(Value::from("in"))),
        Pattern::field("gate", "direction", Condition::Equals(Value::from("out"))),
    );
    gate.add_processor(Processor::new(
        "presence",
        Recipe::per_entity(Group::new("gate", "agent"), inside),
    ))
    .unwrap();
    assert_eq!(gate.derived().count(), 0);

    let mut update = Update::new();
    update.points(
        "gate",
        [
            TimePointEvent::new(1, pass("ada", "in")),
            TimePointEvent::new(4, pass("bob", "in")),
            TimePointEvent::new(9, pass("ada", "out")),
        ],
    );
    let report = gate.update(update).unwrap();
    let changed: Vec<&str> = report.changed().iter().map(|key| key.as_str()).collect();
    assert_eq!(changed, ["presence/ada", "presence/bob"]);
    assert_eq!(bounds(gate.intervals("presence/ada").unwrap()), [(1, 9)]);
    assert!(gate.is_derived("presence/bob"));
    assert_eq!(
        gate.producer("presence/bob")
            .map(|processor| processor.output().as_str()),
        Some("presence")
    );
}

#[test]
fn removing_a_per_entity_processor_removes_its_whole_family_and_nothing_else() {
    let mut gate = StreamSystem::new();
    gate.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    let pass = |agent: &str, direction: &str| {
        Value::record([
            ("agent", Value::from(agent)),
            ("direction", Value::from(direction)),
        ])
    };
    let inside = Formula::between(
        Pattern::field("gate", "direction", Condition::Equals(Value::from("in"))),
        Pattern::field("gate", "direction", Condition::Equals(Value::from("out"))),
    );
    gate.add_processor(Processor::new(
        "presence",
        Recipe::per_entity(Group::new("gate", "agent"), inside),
    ))
    .unwrap();
    gate.add_processor(Processor::new(
        "entering",
        Formula::while_matching(Pattern::field(
            "gate",
            "direction",
            Condition::Equals(Value::from("in")),
        )),
    ))
    .unwrap();
    let mut update = Update::new();
    update.points(
        "gate",
        [
            TimePointEvent::new(1, pass("ada", "in")),
            TimePointEvent::new(4, pass("bob", "in")),
            TimePointEvent::new(9, pass("ada", "out")),
        ],
    );
    gate.update(update).unwrap();
    assert!(gate.is_derived("presence/ada"));
    assert!(gate.is_derived("presence/bob"));

    let entering_before = gate.intervals("entering").cloned();
    let gate_before = gate.points::<Value>("gate").cloned();
    assert!(entering_before.is_some() && gate_before.is_some());

    gate.remove_processor("presence").unwrap();

    assert!(!gate.is_derived("presence/ada"));
    assert!(!gate.is_derived("presence/bob"));
    assert!(gate.intervals("presence/ada").is_none());
    assert!(gate.intervals("presence/bob").is_none());
    assert!(gate.is_derived("entering"));
    assert!(!gate.is_derived("gate"));
    assert_eq!(gate.derived().count(), 1);
    assert_eq!(gate.intervals("entering").cloned(), entering_before);
    assert_eq!(gate.points::<Value>("gate").cloned(), gate_before);
}

#[test]
fn a_processor_reading_only_an_unchanged_output_is_not_evaluated() {
    let mut room = room();
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();

    // A word that is neither start nor end reaches `presence` and changes nothing.
    let report = room.update(motion(&[(30, "noise")])).unwrap();
    assert!(report.evaluated().contains("presence"));
    assert!(!report.changed().contains("presence"));
    // `occupied` reads `presence`, which came out unchanged: it does not run.
    assert!(!report.evaluated().contains("occupied"));
    assert!(!report.changed().contains("occupied"));
    assert_eq!(bounds(room.intervals("presence").unwrap()), [(10, 25)]);
}

#[test]
fn a_changed_output_still_reaches_its_readers() {
    let mut room = room();
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();

    let report = room.update(motion(&[(40, "start"), (50, "end")])).unwrap();
    assert!(report.changed().contains("presence"));
    assert!(report.evaluated().contains("occupied"));
}

#[test]
fn a_per_entity_family_counts_as_changed_when_any_member_changes() {
    let mut gate = StreamSystem::new();
    gate.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .unwrap();
    let pass = |agent: &str, direction: &str| {
        Value::record([
            ("agent", Value::from(agent)),
            ("direction", Value::from(direction)),
        ])
    };
    let inside = Formula::between(
        Pattern::field("gate", "direction", Condition::Equals(Value::from("in"))),
        Pattern::field("gate", "direction", Condition::Equals(Value::from("out"))),
    );
    gate.add_processor(Processor::new(
        "presence",
        Recipe::per_entity(Group::new("gate", "agent"), inside),
    ))
    .unwrap();
    gate.insert_stream("noise", TimePointSequence::<Value>::new("noise"))
        .unwrap();

    let mut update = Update::new();
    update.points("gate", [TimePointEvent::new(1, pass("ada", "in"))]);
    let report = gate.update(update).unwrap();
    assert!(report.changed().contains("presence/ada"));
    gate.add_processor(Processor::new("watched", Formula::state("presence/ada")))
        .unwrap();

    // A member changes: the reader of the family runs.
    let mut update = Update::new();
    update.points("gate", [TimePointEvent::new(9, pass("ada", "out"))]);
    let report = gate.update(update).unwrap();
    assert!(report.changed().contains("presence/ada"));
    assert!(report.evaluated().contains("watched"));

    // An update to another stream does not reach the family's reader.
    let mut update = Update::new();
    update.points("noise", [TimePointEvent::new(3, Value::from("x"))]);
    let report = gate.update(update).unwrap();
    assert!(!report.evaluated().contains("watched"));
}

#[test]
fn a_slice_cuts_the_derived_streams_of_the_whole_system() {
    let mut room = room();
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();

    // The start at 10 lies before the window: presence still holds from 20.
    let window = room.slice(20..45);
    assert_eq!(bounds(window.intervals("presence").unwrap()), [(20, 25)]);
    assert_eq!(
        bounds(window.intervals("occupied").unwrap()),
        [(20, 25), (40, 45)]
    );
    assert_eq!(window.processors(), room.processors());
}

/// The streams the generator's processors read, empty.
fn empty_streams() -> StreamSystem {
    let mut streams = StreamSystem::new();
    for key in POINT_INPUTS {
        streams
            .insert_stream(key, TimePointSequence::<i64>::new(key))
            .unwrap();
    }
    for key in RECORD_INPUTS {
        streams
            .insert_stream(key, TimePointSequence::<Value>::new(key))
            .unwrap();
    }
    for key in STATE_INPUTS {
        streams
            .insert_stream(key, TimeIntervalSequence::new(key))
            .unwrap();
    }
    streams
}

/// The stream's operations cut into updates of a few operations each, each
/// built twice: one for the system, one for the oracle.
fn updates(random: &mut Random, stream: &Stream) -> Vec<(Update, Update)> {
    let fill = |update: &mut Update, op: &Op| match op {
        Op::Point(key, timestamp, value) => {
            update.points(*key, [TimePointEvent::new(*timestamp, *value)]);
        }
        Op::Record(key, timestamp, value) => {
            update.points(*key, [TimePointEvent::new(*timestamp, value.clone())]);
        }
        Op::Span(key, start, end) => {
            update.spans(*key, [span(*start, *end)]);
        }
        // Batch mode has no frontier: a watermark records nothing.
        Op::Advance(key, watermark) => {
            assert!(
                STATE_INPUTS.contains(key)
                    || POINT_INPUTS.contains(key)
                    || RECORD_INPUTS.contains(key)
            );
            assert!(*watermark >= stream.from);
        }
    };

    let mut updates = Vec::new();
    let mut ops = stream.ops.iter().peekable();
    while ops.peek().is_some() {
        let (mut for_system, mut for_oracle) = (Update::new(), Update::new());
        for op in ops.by_ref().take(1 + random.below(4) as usize) {
            fill(&mut for_system, op);
            fill(&mut for_oracle, op);
        }
        updates.push((for_system, for_oracle));
    }
    updates
}

#[test]
fn derived_streams_always_equal_the_processors_run_from_scratch() {
    for_each_seed(property_cases(), |random, _| {
        let list = processors(random);
        let stream = stream(random);

        let mut system = StreamSystem::new();
        let mut oracle = empty_streams();
        for key in POINT_INPUTS
            .iter()
            .chain(&RECORD_INPUTS)
            .chain(&STATE_INPUTS)
        {
            let sequence = oracle.get(key).expect("filled above").clone();
            system.insert_stream(*key, sequence).unwrap();
        }
        for processor in &list {
            system.add_processor(processor.clone()).unwrap();
        }

        for (update, for_oracle) in updates(random, &stream) {
            let previous = system.clone();
            oracle.update(for_oracle).unwrap();
            let report = system.update(update).unwrap();

            // From scratch: every processor added to the streams as they are.
            let expected = update::run(&oracle, &list).unwrap();
            let derived: Vec<&str> = system.derived().map(|key| key.as_str()).collect();
            let wanted: Vec<&str> = expected.derived().map(|key| key.as_str()).collect();
            assert_eq!(derived, wanted);
            for key in expected.derived() {
                assert_eq!(
                    system.get(key.as_str()),
                    expected.get(key.as_str()),
                    "{key}"
                );
                let moved = previous.get(key.as_str()) != expected.get(key.as_str());
                assert_eq!(report.changed().contains(key), moved, "{key}");
            }
        }

        // Narrowing every stream to the window the stream was observed over
        // evaluates every processor again, as a run from scratch would.
        if stream.from < stream.horizon {
            let window = Extent::from_spans(vec![span(stream.from, stream.horizon)]);
            system.observe(&window).unwrap();
            oracle.observe(&window).unwrap();
            let expected = update::run(&oracle, &list).unwrap();
            for key in expected.derived() {
                assert_eq!(
                    system.get(key.as_str()),
                    expected.get(key.as_str()),
                    "{key}"
                );
            }
        }
    });
}

/// Motion readings, with noise, cleaned by a transform of the stream itself,
/// and presence derived from the cleaned stream.
fn cleaned_room() -> StreamSystem {
    let mut room = StreamSystem::new();
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.add_processor(Processor::new("motion", drop_noise()))
        .unwrap();
    room.add_processor(Processor::new("presence", presence()))
        .unwrap();
    room
}

fn drop_noise() -> Events {
    Events::Matching(Pattern::new("motion", Condition::NotEquals("noise")))
}

/// The timestamps of the points of `key`.
fn instants(system: &StreamSystem, key: &str) -> Vec<i64> {
    system
        .points::<&str>(key)
        .unwrap()
        .iter()
        .map(|event| event.timestamp())
        .collect()
}

#[test]
fn a_transform_rewrites_its_stream_and_every_reader_sees_the_last_version() {
    let mut room = cleaned_room();
    let report = room
        .update(motion(&[
            (5, "noise"),
            (10, "start"),
            (15, "noise"),
            (25, "end"),
        ]))
        .unwrap();

    // The stream reads as its cleaned version, the recorded data is kept.
    assert_eq!(instants(&room, "motion"), [10, 25]);
    let raw = room.raw("motion").unwrap().as_time_point::<&str>().unwrap();
    assert_eq!(raw.len(), 4);
    assert_eq!(room.intervals("presence").unwrap()[0].bounds(), (10, 25));
    assert!(report.changed().contains("motion") && report.changed().contains("presence"));

    // It is still a stream, not a derived one.
    assert!(!room.is_derived("motion") && room.is_transformed("motion"));
    assert_eq!(room.streams().count(), 1);
    assert_eq!(room.derived().count(), 1);
    assert_eq!(room.transforms("motion").len(), 1);
    assert_eq!(
        room.producer("motion")
            .map(|processor| processor.output().as_str()),
        Some("motion")
    );
    assert!(room.raw("presence").is_none());
    assert!(room.transforms("presence").is_empty());

    // More recorded data runs the chain again; noise alone changes nothing.
    let report = room.update(motion(&[(30, "noise")])).unwrap();
    assert!(report.changed().is_empty());
    assert_eq!(room.raw("motion").unwrap().len(), 5);
    assert_eq!(instants(&room, "motion"), [10, 25]);
}

#[test]
fn a_stream_nothing_transforms_has_its_recorded_data_as_raw() {
    let room = room();
    assert_eq!(room.raw("motion"), room.get("motion"));
    assert_eq!(room.raw("presence"), None);
    assert_eq!(room.raw("nothing"), None);
    assert!(!room.is_transformed("motion"));
}

#[test]
fn transforms_of_one_stream_chain_in_the_order_they_are_added() {
    let mut room = StreamSystem::new();
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.add_processor(Processor::new("motion", drop_noise()))
        .unwrap();
    room.add_processor(Processor::new("motion", Events::changes("motion")))
        .unwrap();
    room.add_processor(Processor::new("presence", presence()))
        .unwrap();

    room.update(motion(&[
        (1, "start"),
        (2, "noise"),
        (3, "start"),
        (4, "end"),
        (5, "end"),
    ]))
    .unwrap();

    // Noise dropped, then the repeats of what is left.
    assert_eq!(instants(&room, "motion"), [1, 4]);
    assert_eq!(room.raw("motion").unwrap().len(), 5);
    assert_eq!(room.intervals("presence").unwrap()[0].bounds(), (1, 4));
    assert_eq!(room.transforms("motion").len(), 2);

    // Removing from the end brings back the version before.
    room.remove_processor("presence").unwrap();
    room.remove_processor("motion").unwrap();
    assert_eq!(instants(&room, "motion"), [1, 3, 4, 5]);
    assert_eq!(room.transforms("motion").len(), 1);
    room.remove_processor("motion").unwrap();
    assert!(!room.is_transformed("motion"));
    assert_eq!(room.get("motion"), room.raw("motion"));
    assert_eq!(room.get("motion").unwrap().len(), 5);
    assert!(room.remove_stream("motion").unwrap().is_some());
}

#[test]
fn a_transform_refuses_what_would_leave_the_system_inconsistent() {
    let mut room = cleaned_room();
    room.update(motion(&[(10, "start"), (25, "end")])).unwrap();
    let before = room.clone();

    // A transform that changes the shape of the stream.
    assert!(matches!(
        room.add_processor(Processor::new(
            "door",
            Events::Edges(Formula::state("door"))
        )),
        Err(UpdateError::Process { .. })
    ));
    let mut door = StreamSystem::new();
    door.insert_stream(
        "door",
        TimeIntervalSequence::from_spans("door", vec![span(1, 2)]),
    )
    .unwrap();
    let before_door = door.clone();
    assert!(matches!(
        door.add_processor(Processor::new(
            "door",
            Events::Edges(Formula::state("door"))
        )),
        Err(UpdateError::Process {
            error: ProcessError::TransformShape { .. },
            ..
        })
    ));
    assert_eq!(door, before_door);

    // A reader of the stream already reads a version: no more transforms.
    assert_eq!(
        room.add_processor(Processor::new("motion", Events::changes("motion"))),
        Err(UpdateError::Read {
            key: "motion".into(),
            by: "presence".into()
        })
    );
    // A derived stream is not rewritten, and a missing one is no stream.
    assert!(matches!(
        room.add_processor(Processor::new("presence", Formula::state("presence"))),
        Err(UpdateError::Taken { .. })
    ));
    assert!(matches!(
        room.add_processor(Processor::new("missing", Formula::state("missing"))),
        Err(UpdateError::Process {
            error: ProcessError::UnknownKey { .. },
            ..
        })
    ));
    // What a reader reads cannot be taken out, the transform included.
    assert_eq!(
        room.remove_processor("motion"),
        Err(UpdateError::Read {
            key: "motion".into(),
            by: "presence".into()
        })
    );
    assert!(matches!(
        room.remove_stream("motion"),
        Err(UpdateError::Read { .. })
    ));
    assert_eq!(room, before);

    // A chain of two cannot lose its first transform while the second reads it.
    let mut chain = StreamSystem::new();
    chain
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    chain
        .add_processor(Processor::new("motion", drop_noise()))
        .unwrap();
    chain
        .add_processor(Processor::new("motion", Events::changes("motion")))
        .unwrap();
    chain.update(motion(&[(1, "start"), (2, "start")])).unwrap();
    let kept = chain.clone();
    // Removal by key takes the last transform, never one in the middle.
    let removed = chain.remove_processor("motion").unwrap();
    assert_eq!(removed, Processor::new("motion", Events::changes("motion")));
    assert_eq!(chain.transforms("motion").len(), 1);
    assert_ne!(chain, kept);
}

#[test]
fn recording_into_a_transformed_stream_records_into_the_recorded_data() {
    let mut room = cleaned_room();
    room.set_schema("motion", Schema::Integer).unwrap_err();

    let mut wrong = Update::new();
    wrong.points("motion", [TimePointEvent::new(1, 7_i64)]);
    let before = room.clone();
    assert!(matches!(room.update(wrong), Err(UpdateError::Record(_))));
    assert_eq!(room, before);

    room.relabel("motion", "movement").unwrap();
    assert_eq!(room.raw("motion").unwrap().label(), "movement");
    room.insert_stream(
        "motion",
        TimePointSequence::from_events(
            "motion",
            vec![
                TimePointEvent::new(3, "noise"),
                TimePointEvent::new(4, "start"),
            ],
        ),
    )
    .unwrap();
    assert_eq!(instants(&room, "motion"), [4]);
    assert_eq!(room.raw("motion").unwrap().len(), 2);
}

/// One transform of a stream, by what it reads and what it keeps.
#[derive(Clone, Copy, Debug)]
enum Transform {
    /// Points below a number.
    Below(i64),
    /// Points above a number.
    Above(i64),
    /// Points that differ from the one before.
    Changes,
    /// A timeline moved by a number.
    Shifted(i64),
    /// A timeline joined with another timeline.
    Joined,
    /// A timeline restricted to the other timeline.
    Restricted,
}

impl Transform {
    fn processor(self, output: &str, source: &str) -> Processor {
        match self {
            Self::Below(bound) => Processor::new(
                output,
                Events::Matching(Pattern::new(source, Condition::Less(bound))),
            ),
            Self::Above(bound) => Processor::new(
                output,
                Events::Matching(Pattern::new(source, Condition::Greater(bound))),
            ),
            Self::Changes => Processor::new(output, Events::changes(source)),
            Self::Shifted(by) => Processor::new(output, Formula::state(source).shift(by, by)),
            Self::Joined => Processor::new(output, Formula::state(source).or(Formula::state("t"))),
            Self::Restricted => {
                Processor::new(output, Formula::state(source).and(Formula::state("t")))
            }
        }
    }
}

/// A random chain of transforms for the stream of points `p` or the timeline `s`.
fn chain(random: &mut Random, points: bool) -> Vec<Transform> {
    (0..1 + random.below(3))
        .map(|_| match (points, random.below(3)) {
            (true, 0) => Transform::Below(random.between(1, 6)),
            (true, 1) => Transform::Above(random.between(-1, 4)),
            (true, _) => Transform::Changes,
            (false, 0) => Transform::Shifted(random.between(-3, 4)),
            (false, 1) => Transform::Joined,
            (false, _) => Transform::Restricted,
        })
        .collect()
}

/// The streams `p`, `s` and `t`, empty.
fn plain_streams() -> StreamSystem {
    let mut system = StreamSystem::new();
    system
        .insert_stream("p", TimePointSequence::<i64>::new("p"))
        .unwrap();
    system
        .insert_stream("s", TimeIntervalSequence::new("s"))
        .unwrap();
    system
        .insert_stream("t", TimeIntervalSequence::new("t"))
        .unwrap();
    system
}

/// What a stream holds, without its label: the points as pairs, a timeline as
/// its bounds.
fn content(system: &StreamSystem, key: &str) -> Vec<(i64, i64)> {
    system.points::<i64>(key).map_or_else(
        || bounds(system.intervals(key).unwrap()),
        |points| {
            points
                .iter()
                .map(|event| (event.timestamp(), *event.value()))
                .collect()
        },
    )
}

/// What every reader reads: the last version of each stream.
fn readers(source_p: &str, source_s: &str) -> [Processor; 2] {
    [
        Processor::new(
            "seen",
            Formula::occurs(Pattern::new(source_p, Condition::<i64>::Any)),
        ),
        Processor::new("open", Formula::state(source_s).or(Formula::state("t"))),
    ]
}

#[test]
fn transformed_streams_always_equal_the_chain_run_from_scratch() {
    for_each_seed(property_cases(), |random, _| {
        let for_p = chain(random, true);
        let for_s = chain(random, false);

        // The system rewrites `p` and `s` in place.
        let mut system = plain_streams();
        for transform in &for_p {
            system.add_processor(transform.processor("p", "p")).unwrap();
        }
        for transform in &for_s {
            system.add_processor(transform.processor("s", "s")).unwrap();
        }
        for reader in readers("p", "s") {
            system.add_processor(reader).unwrap();
        }

        // The oracle names every version: `p1` reads `p`, `p2` reads `p1`.
        let mut oracle = plain_streams();
        let (mut last_p, mut last_s) = ("p".to_owned(), "s".to_owned());
        for (stem, list, last) in [("p", &for_p, &mut last_p), ("s", &for_s, &mut last_s)] {
            for (position, transform) in list.iter().enumerate() {
                let name = format!("{stem}{}", position + 1);
                oracle
                    .add_processor(transform.processor(&name, last))
                    .unwrap();
                *last = name;
            }
        }
        for reader in readers(&last_p, &last_s) {
            oracle.add_processor(reader).unwrap();
        }

        for _ in 0..1 + random.below(5) {
            let mut for_system = Update::new();
            let mut for_oracle = Update::new();
            for _ in 0..random.below(5) {
                let event = TimePointEvent::new(random.between(0, 60), random.between(0, 6));
                for_system.points("p", [event]);
                for_oracle.points("p", [event]);
            }
            for key in ["s", "t"] {
                if random.chance(2, 3) {
                    let start = random.between(0, 60);
                    let end = start + random.between(1, 10);
                    for_system.spans(key, [span(start, end)]);
                    for_oracle.spans(key, [span(start, end)]);
                }
            }
            system.update(for_system).unwrap();
            oracle.update(for_oracle).unwrap();

            assert_eq!(
                content(&system, "p"),
                content(&oracle, &last_p),
                "{for_p:?}"
            );
            assert_eq!(
                content(&system, "s"),
                content(&oracle, &last_s),
                "{for_s:?}"
            );
            assert_eq!(system.raw("p"), oracle.get("p"));
            assert_eq!(system.raw("s"), oracle.get("s"));
            assert_eq!(
                content(&system, "seen"),
                content(&oracle, "seen"),
                "{for_p:?}"
            );
            assert_eq!(
                content(&system, "open"),
                content(&oracle, "open"),
                "{for_s:?}"
            );
        }
    });
}

#[test]
fn a_slice_and_an_observation_keep_the_recorded_data_and_the_versions_apart() {
    let mut room = cleaned_room();
    room.update(motion(&[(5, "noise"), (10, "start"), (25, "end")]))
        .unwrap();

    let window = room.slice(8..30);
    assert!(window.is_transformed("motion"));
    assert_eq!(instants(&window, "motion"), [10, 25]);
    assert_eq!(window.raw("motion").unwrap().len(), 2);
    assert_eq!(window.transforms("motion").len(), 1);

    // Observing narrows the recorded data and the versions follow.
    room.observe(&Extent::from_spans(vec![span(8, 30)]))
        .unwrap();
    assert_eq!(room.raw("motion").unwrap().len(), 2);
    assert_eq!(instants(&room, "motion"), [10, 25]);
    assert_eq!(room.intervals("presence").unwrap()[0].bounds(), (10, 25));
}

#[test]
fn the_producer_of_a_transformed_stream_is_its_last_transform_and_the_chain_is_transforms() {
    let mut room = StreamSystem::new();
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    assert!(room.producer("motion").is_none());

    room.add_processor(Processor::new("motion", drop_noise()))
        .unwrap();
    let first = room.producer("motion").unwrap().clone();
    room.add_processor(Processor::new("motion", Events::changes("motion")))
        .unwrap();
    let last = room.transforms("motion").pop().unwrap().clone();
    assert_ne!(first, last);

    for system in [room.clone(), room.slice(0..5)] {
        assert_eq!(system.producer("motion"), Some(&last));
        assert_eq!(system.transforms("motion").len(), 2);
        assert_eq!(system.transforms("motion")[0], &first);
    }

    // Removing the last transform makes the one before the producer.
    room.remove_processor("motion").unwrap();
    assert_eq!(room.producer("motion"), Some(&first));
    room.remove_processor("motion").unwrap();
    assert!(room.producer("motion").is_none());
}

#[test]
fn a_reader_of_an_earlier_version_refuses_a_later_transform_and_a_removal() {
    let mut room = StreamSystem::new();
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.add_processor(Processor::new("motion", drop_noise()))
        .unwrap();
    room.add_processor(Processor::new("motion", Events::changes("motion")))
        .unwrap();
    room.add_processor(Processor::new("presence", presence()))
        .unwrap();
    let before = room.clone();

    // Neither the last transform nor the stream can go while a reader reads it.
    assert!(matches!(
        room.remove_processor("motion"),
        Err(UpdateError::Read { .. })
    ));
    assert!(matches!(
        room.remove_stream("motion"),
        Err(UpdateError::Read { .. })
    ));
    assert_eq!(room, before);

    // Without the reader, the chain unwinds to the recorded data.
    room.remove_processor("presence").unwrap();
    room.remove_processor("motion").unwrap();
    room.remove_processor("motion").unwrap();
    assert!(room.remove_processor("motion").is_err());
    assert!(room.transforms("motion").is_empty());
}

#[test]
fn evaluating_a_transform_once_reads_the_last_version_and_changes_nothing() {
    let mut room = cleaned_room();
    room.update(motion(&[(5, "noise"), (10, "start"), (25, "end")]))
        .unwrap();
    let before = room.clone();

    // Another transform, not added: it reads the cleaned version, not the recorded data.
    let changes = Processor::new("motion", Events::changes("motion"));
    let evaluated = changes.evaluate(&room).unwrap();
    assert_eq!(evaluated.len(), 2);

    // The transform already in the chain, run once more, reads the last version.
    let again = Processor::new("motion", drop_noise())
        .evaluate(&room)
        .unwrap();
    assert_eq!(again.len(), 2);
    assert_eq!(room, before);

    // Over a stream nothing transforms, it reads the stream itself.
    let mut plain = StreamSystem::new();
    plain
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    plain
        .update(motion(&[(5, "noise"), (10, "start")]))
        .unwrap();
    assert_eq!(
        Processor::new("motion", drop_noise())
            .evaluate(&plain)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn replacing_a_transformed_stream_resets_its_metadata_as_for_a_plain_stream() {
    let mut room = cleaned_room();
    room.member_metadata_mut("motion")
        .unwrap()
        .insert("unit".into(), "events".into());
    room.update(motion(&[(10, "start")])).unwrap();
    // Following a change keeps it.
    assert!(room
        .member_metadata("motion")
        .and_then(|metadata| metadata.get("unit"))
        .is_some());

    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    assert!(room
        .member_metadata("motion")
        .is_none_or(|metadata| metadata.get("unit").is_none()));
    assert!(room.is_transformed("motion"));
    assert_eq!(room.raw("motion").unwrap().len(), 0);

    let mut plain = StreamSystem::new();
    plain
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    plain
        .member_metadata_mut("motion")
        .unwrap()
        .insert("unit".into(), "events".into());
    plain
        .insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    assert!(plain
        .member_metadata("motion")
        .is_none_or(|metadata| metadata.get("unit").is_none()));
}
