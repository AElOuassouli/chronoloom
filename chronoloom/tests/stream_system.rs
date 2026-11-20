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
    Condition, Extent, Formula, Group, Pattern, ProcessError, Recipe, Schema, SequenceSystem,
    TimeIntervalSequence, TimePointEvent, TimePointSequence, Value,
};

use common::{bounds, for_each_seed, span, Random};
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
            error: ProcessError::Cycle { .. },
            ..
        })
    ));
    assert!(matches!(
        room.add_processor(Processor::new("later", Formula::state("missing"))),
        Err(UpdateError::Process { .. })
    ));
    // An output key already in use, by a stream or another processor.
    for taken in ["door", "presence"] {
        assert_eq!(
            room.add_processor(Processor::new(taken, Formula::state("door"))),
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
fn empty_streams() -> SequenceSystem {
    let mut streams = SequenceSystem::new();
    for key in POINT_INPUTS {
        streams.insert(key, TimePointSequence::<i64>::new(key));
    }
    for key in RECORD_INPUTS {
        streams.insert(key, TimePointSequence::<Value>::new(key));
    }
    for key in STATE_INPUTS {
        streams.insert(key, TimeIntervalSequence::new(key));
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
    for_each_seed(300, |random, _| {
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
            oracle.record_update(for_oracle).unwrap();
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
            oracle.observe(&window);
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
