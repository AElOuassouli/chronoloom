//! Declared inputs and `Processor::validate`: every problem of a processor
//! listed in a stable order, and a stream system, in either mode, held to
//! the declarations.

#[path = "common/update.rs"]
mod update;

use chronoloom::{
    Condition, Events, Formula, Group, Input, Kind, Live, Pattern, ProcessError, Processor, Recipe,
    Schema, StreamSystem, TimeIntervalEvent, TimeIntervalSequence, TimePointEvent,
    TimePointSequence, Update, UpdateError, Value,
};

fn schema() -> Schema {
    Schema::record([
        ("agent", Schema::Text),
        ("count", Schema::Integer),
        ("site", Schema::record([("zone", Schema::Text)])),
    ])
}

fn in_output(output: &str, error: ProcessError) -> ProcessError {
    ProcessError::InOutput {
        output: output.into(),
        error: Box::new(error),
    }
}

/// A processor of `output` that declares the three inputs of the world.
fn declared(output: &str, formula: impl Into<Recipe>) -> Processor {
    Processor::new(output, formula)
        .with_input("temperature", Input::Points(Kind::Float))
        .with_input("door", Input::Timeline)
        .with_input("records", Input::Records(schema()))
}

#[test]
fn a_valid_processor_validates_with_and_without_declarations() {
    let processors = [
        declared(
            "hot",
            Formula::while_matching(Pattern::new("temperature", Condition::Greater(5.0))),
        ),
        // Reading another processor's output: declared like any other input.
        declared("open", Formula::state("door").or(Formula::state("hot")))
            .with_input("hot", Input::Timeline),
        declared(
            "busy",
            Formula::while_matching(Pattern::field(
                "records",
                "site.zone",
                Condition::Equals(Value::from("gate")),
            )),
        ),
        declared("counts", Events::project("records", "count", Kind::Float)),
        declared(
            "each",
            Recipe::per_entity(Group::new("records", "agent"), Formula::state("door")),
        ),
    ];
    for processor in &processors {
        assert_eq!(processor.validate(), Ok(()), "{processor}");
    }

    // Without declarations, nothing is known about the keys that are read.
    let bare = Processor::new("later", Formula::state("each/agent-7"));
    assert_eq!(bare.validate(), Ok(()));
}

#[test]
fn validate_lists_every_problem_of_a_processor_in_formula_order() {
    let float = std::any::type_name::<f64>();
    let integer = std::any::type_name::<i64>();
    let cases = [
        (
            declared("shadowed", Formula::state("door")).with_input("shadowed", Input::Timeline),
            vec![ProcessError::InputIsOutput {
                key: "shadowed".into(),
            }],
        ),
        (
            declared(
                "a",
                Formula::while_matching(Pattern::new("door", Condition::Equals(1_i64))).or(
                    Formula::occurs(Pattern::new("temperature", Condition::Any::<i64>)),
                ),
            ),
            vec![
                in_output("a", ProcessError::NotPoints { key: "door".into() }),
                in_output(
                    "a",
                    ProcessError::ValueTypeMismatch {
                        key: "temperature".into(),
                        expected: float,
                        found: integer,
                    },
                ),
            ],
        ),
        (
            // Two problems in one formula, left to right.
            declared(
                "b",
                Formula::state("temperature").and(Formula::state("missing")),
            ),
            vec![
                in_output(
                    "b",
                    ProcessError::NotATimeline {
                        key: "temperature".into(),
                    },
                ),
                in_output(
                    "b",
                    ProcessError::UnknownKey {
                        key: "missing".into(),
                    },
                ),
            ],
        ),
        (
            declared(
                "c",
                Recipe::per_entity(
                    Group::new("records", "nobody"),
                    Formula::while_matching(Pattern::field(
                        "records",
                        "agent",
                        Condition::Equals(Value::Integer(3)),
                    )),
                ),
            ),
            vec![
                in_output(
                    "c",
                    ProcessError::MissingField {
                        key: "records".into(),
                        path: "nobody".into(),
                    },
                ),
                in_output(
                    "c",
                    ProcessError::FieldMismatch {
                        key: "records".into(),
                        path: "agent".into(),
                        expected: "text".into(),
                        found: "integer",
                    },
                ),
            ],
        ),
        (
            declared("d", Events::project("records", "agent", Kind::Integer)),
            vec![in_output(
                "d",
                ProcessError::ProjectionMismatch {
                    key: "records".into(),
                    path: "agent".into(),
                    expected: Kind::Integer,
                    found: "text",
                },
            )],
        ),
        (
            declared("e", Events::project("records", "site.street", Kind::Text)),
            vec![in_output(
                "e",
                ProcessError::MissingField {
                    key: "records".into(),
                    path: "site.street".into(),
                },
            )],
        ),
    ];

    for (processor, problems) in cases {
        assert_eq!(processor.validate(), Err(problems), "{processor}");
    }
    // An integer field widens into a float projection.
    assert_eq!(
        declared("f", Events::project("records", "count", Kind::Float)).validate(),
        Ok(())
    );
}

#[test]
fn a_transform_is_a_valid_processor() {
    // Reading its own output, declared or not, is what a transform does.
    let plain = Processor::new("a", Formula::state("a"));
    assert_eq!(plain.validate(), Ok(()));
    assert_eq!(plain.check(), Ok(()));

    let with_declaration = declared("a", Formula::state("a")).with_input("a", Input::Timeline);
    assert_eq!(with_declaration.validate(), Ok(()));
    assert_eq!(with_declaration.check(), Ok(()));

    // Its other reads are still checked, and agree with `check`.
    let processor = declared("a", Formula::state("a").or(Formula::state("nowhere")));
    let problems = processor.validate().unwrap_err();
    assert_eq!(Err(problems[0].clone()), processor.check());
    assert!(problems
        .iter()
        .all(|problem| !matches!(problem, ProcessError::Cycle { .. })));
}

#[test]
fn check_reports_the_first_problem_validate_lists() {
    let processors = [
        declared("a", Formula::state("nowhere").or(Formula::state("missing"))),
        declared("door", Formula::state("nowhere")),
    ];
    for processor in processors {
        let problems = processor.validate().unwrap_err();
        assert!(problems.len() > 1, "{problems:?}");
        assert_eq!(Err(problems[0].clone()), processor.check(), "{problems:?}");
    }
}

#[test]
fn a_path_through_an_any_shape_is_not_missing() {
    let processor = Processor::new(
        "a",
        Formula::occurs(Pattern::field("loose", "data.deep.er", Condition::Any)),
    )
    .with_input(
        "loose",
        Input::Records(Schema::record([("data", Schema::Any)])),
    );

    assert_eq!(processor.validate(), Ok(()));
}

fn world() -> StreamSystem {
    let mut system = StreamSystem::new();
    system
        .insert_stream(
            "temperature",
            TimePointSequence::from_events("temperature", vec![TimePointEvent::new(1, 2.0_f64)]),
        )
        .unwrap();
    system
        .insert_stream(
            "door",
            TimeIntervalSequence::from_spans("door", vec![TimeIntervalEvent::span(0, 5).unwrap()]),
        )
        .unwrap();
    system
        .insert_stream(
            "records",
            TimePointSequence::from_events(
                "records",
                vec![TimePointEvent::new(
                    2,
                    Value::record([
                        ("agent", Value::from("a")),
                        ("count", Value::from(1)),
                        ("site", Value::record([("zone", Value::from("z"))])),
                    ]),
                )],
            ),
        )
        .unwrap();
    system
}

#[test]
fn a_stream_system_holds_its_streams_to_the_declarations() {
    let processor = [declared("open", Formula::state("door"))];
    assert!(update::run(&world(), &processor).is_ok());

    let mut missing = world();
    missing.remove_stream("door").unwrap();
    assert_eq!(
        update::run(&missing, &processor).err(),
        Some(ProcessError::UnknownKey { key: "door".into() })
    );

    let mut flipped = world();
    flipped
        .insert_stream("door", TimePointSequence::<f64>::new("door"))
        .unwrap();
    assert_eq!(
        update::run(&flipped, &processor).err(),
        Some(ProcessError::NotATimeline { key: "door".into() })
    );

    let mut flipped = world();
    flipped
        .insert_stream("temperature", TimeIntervalSequence::new("temperature"))
        .unwrap();
    assert_eq!(
        update::run(&flipped, &processor).err(),
        Some(ProcessError::NotPoints {
            key: "temperature".into()
        })
    );

    let mut integers = world();
    integers
        .insert_stream("temperature", TimePointSequence::<i64>::new("temperature"))
        .unwrap();
    assert!(matches!(
        update::run(&integers, &processor),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "temperature"
    ));

    let mut stray = world();
    stray
        .insert_stream(
            "records",
            TimePointSequence::from_events(
                "records",
                vec![TimePointEvent::new(
                    2,
                    Value::record([("agent", Value::from(1))]),
                )],
            ),
        )
        .unwrap();
    assert!(matches!(
        update::run(&stray, &processor),
        Err(ProcessError::SchemaMismatch { key, .. }) if key.as_str() == "records"
    ));

    // Nothing is added when the streams do not satisfy a declaration.
    let mut system = update::run(&missing, &[]).expect("streams alone");
    let before = system.clone();
    assert!(system.add_processor(processor[0].clone()).is_err());
    assert_eq!(system, before);
}

#[test]
fn a_processor_without_declarations_reads_what_its_formula_reads() {
    let processor = [Processor::new("open", Formula::state("door"))];
    let mut system = StreamSystem::new();
    system
        .insert_stream("door", TimeIntervalSequence::new("door"))
        .unwrap();

    assert!(update::run(&system, &processor).is_ok());
    assert_eq!(
        update::run(&StreamSystem::new(), &processor).err(),
        Some(ProcessError::UnknownKey { key: "door".into() })
    );
}

/// A live system holding the three streams of the world, empty.
fn live_world() -> StreamSystem<Live> {
    let mut live = StreamSystem::live(0);
    live.insert_stream("temperature", TimePointSequence::<f64>::new("temperature"))
        .unwrap();
    live.insert_stream("door", TimeIntervalSequence::from_spans("door", Vec::new()))
        .unwrap();
    live.insert_stream("records", TimePointSequence::<Value>::new("records"))
        .unwrap();
    live
}

#[test]
fn live_refuses_reads_that_contradict_the_declarations() {
    let mut live = live_world();
    let before = live.clone();

    let contradicting = declared(
        "a",
        Formula::while_matching(Pattern::new("temperature", Condition::Greater(5_i64))),
    );
    assert!(matches!(
        live.add_processor(contradicting),
        Err(UpdateError::Process {
            error: ProcessError::ValueTypeMismatch { key, .. },
            ..
        }) if key.as_str() == "temperature"
    ));

    let shape = declared("a", Formula::state("temperature"));
    assert!(matches!(
        live.add_processor(shape),
        Err(UpdateError::Process {
            error: ProcessError::NotATimeline { key },
            ..
        }) if key.as_str() == "temperature"
    ));

    let shadowed = declared("door", Formula::state("temperature"));
    assert!(matches!(
        live.add_processor(shadowed),
        Err(UpdateError::Taken { key }) if key.as_str() == "door"
    ));
    assert_eq!(live, before);
}

#[test]
fn live_holds_the_streams_to_the_declarations_when_a_processor_is_added() {
    // `door` is declared and read by nothing, and still an input that must be there.
    let mut missing = StreamSystem::live(0);
    missing
        .insert_stream("temperature", TimePointSequence::<f64>::new("temperature"))
        .unwrap();
    assert!(matches!(
        missing.add_processor(declared("changes", Events::changes("temperature"))),
        Err(UpdateError::Process {
            error: ProcessError::UnknownKey { key },
            ..
        }) if key.as_str() == "door"
    ));

    // A record off the declared schema is refused when it is fed, and nothing
    // is recorded.
    let mut live = live_world();
    live.add_processor(declared("changes", Events::changes("temperature")))
        .unwrap();
    let before = live.clone();
    let mut stray_update = Update::new();
    stray_update
        .points(
            "records",
            [TimePointEvent::new(
                1,
                Value::record([("agent", Value::from(1))]),
            )],
        )
        .advance("records", 2);
    assert!(matches!(
        live.update(stray_update),
        Err(UpdateError::Process {
            error: ProcessError::SchemaMismatch { key, .. },
            ..
        }) if key.as_str() == "records"
    ));
    assert_eq!(live, before);

    // A stream whose records are not of the declared schema is refused.
    let stray = Value::record([("agent", Value::from(1))]);
    let mut off_schema = StreamSystem::live(0);
    off_schema
        .insert_stream("temperature", TimePointSequence::<f64>::new("temperature"))
        .unwrap();
    off_schema
        .insert_stream("door", TimeIntervalSequence::from_spans("door", Vec::new()))
        .unwrap();
    off_schema
        .insert_stream(
            "records",
            TimePointSequence::from_events("records", vec![TimePointEvent::new(1, stray)]),
        )
        .unwrap();
    assert!(matches!(
        off_schema.add_processor(declared("changes", Events::changes("temperature"))),
        Err(UpdateError::Process {
            error: ProcessError::SchemaMismatch { key, .. },
            ..
        }) if key.as_str() == "records"
    ));
}

#[test]
fn evaluating_a_processor_once_holds_the_system_to_the_declarations_and_stores_nothing() {
    let processor = declared("open", Formula::state("door"));
    let system = world();

    let sequence = processor.evaluate(&system).expect("the door is a timeline");
    assert_eq!(
        sequence.as_time_interval().map(TimeIntervalSequence::len),
        Some(1)
    );
    assert!(!system.contains("open"));

    let mut flipped = world();
    flipped
        .insert_stream("door", TimePointSequence::<f64>::new("door"))
        .unwrap();
    assert_eq!(
        processor.evaluate(&flipped),
        Err(ProcessError::NotATimeline { key: "door".into() })
    );
}

#[test]
fn evaluating_a_processor_once_names_a_missing_or_shadowed_input() {
    let system = world();

    let missing =
        Processor::new("open", Formula::state("door")).with_input("ghost", Input::Timeline);
    assert_eq!(
        missing.evaluate(&system),
        Err(ProcessError::UnknownKey {
            key: "ghost".into()
        })
    );

    let shadowed =
        Processor::new("door", Formula::state("door")).with_input("door", Input::Timeline);
    assert!(shadowed.evaluate(&system).is_ok());

    // Reading its own output is a transform: evaluated over the stream.
    assert!(Processor::new("door", Formula::state("door"))
        .evaluate(&system)
        .is_ok());

    // Without declarations the recipe alone is evaluated, and an unread key is an error of the recipe.
    assert!(Processor::new("open", Formula::state("door"))
        .evaluate(&system)
        .is_ok());
    assert!(Processor::new("open", Formula::state("nothing"))
        .evaluate(&system)
        .is_err());
}
