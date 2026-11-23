//! Declared inputs and `Processor::validate`: every problem of a processor
//! listed in a stable order, and a stream system or a live processor held to
//! the declarations.

#[path = "common/update.rs"]
mod update;

use chronoloom::{
    Condition, Events, Formula, Group, Input, Kind, LiveProcessor, Pattern, ProcessError,
    Processor, Recipe, Schema, StreamSystem, TimeIntervalEvent, TimeIntervalSequence,
    TimePointEvent, TimePointSequence, Value,
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
fn validate_agrees_with_check_on_a_formula_that_reads_its_own_output() {
    let processor = declared("a", Formula::state("a").or(Formula::state("nowhere")));

    let problems = processor.validate().unwrap_err();
    assert_eq!(Err(problems[0].clone()), processor.check());
    assert!(matches!(problems[0], ProcessError::Cycle { .. }));
    assert_eq!(problems.len(), 2, "{problems:?}");
}

#[test]
fn check_reports_the_first_problem_validate_lists() {
    let processors = [
        declared("a", Formula::state("a").or(Formula::state("nowhere"))),
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

#[test]
fn live_refuses_reads_that_contradict_the_declarations() {
    let contradicting = [declared(
        "a",
        Formula::while_matching(Pattern::new("temperature", Condition::Greater(5_i64))),
    )];
    assert!(matches!(
        LiveProcessor::new(&contradicting, 0),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "temperature"
    ));

    let shape = [declared("a", Formula::state("temperature"))];
    assert_eq!(
        LiveProcessor::new(&shape, 0).err(),
        Some(ProcessError::NotATimeline {
            key: "temperature".into()
        })
    );

    let shadowed = [declared("door", Formula::state("temperature"))];
    assert_eq!(
        LiveProcessor::new(&shadowed, 0).err(),
        Some(ProcessError::InputIsOutput { key: "door".into() })
    );
}

#[test]
fn live_creates_the_declared_inputs_and_refuses_records_off_the_schema() {
    let processor = [declared("changes", Events::changes("temperature"))];
    let mut live = LiveProcessor::new(&processor, 0).unwrap();

    // `door` is declared and read by nothing, and still an input.
    live.on_span("door", TimeIntervalEvent::span(1, 2).unwrap())
        .unwrap();
    // The type of `temperature` comes from the declaration, not from the read.
    assert!(matches!(
        live.on_point("temperature", TimePointEvent::new(1, 5_i64)),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    live.on_point("temperature", TimePointEvent::new(1, 5.0_f64))
        .unwrap();

    let stray = Value::record([("agent", Value::from(1))]);
    assert!(matches!(
        live.on_point("records", TimePointEvent::new(1, stray)),
        Err(ProcessError::SchemaMismatch { key, .. }) if key.as_str() == "records"
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
    assert_eq!(
        shadowed.evaluate(&system),
        Err(ProcessError::InputIsOutput { key: "door".into() })
    );

    // Reading its own output is refused without any declaration, as adding it is.
    assert_eq!(
        Processor::new("door", Formula::state("door")).evaluate(&system),
        Err(ProcessError::Cycle {
            keys: vec!["door".into()]
        })
    );

    // Without declarations the recipe alone is evaluated, and an unread key is an error of the recipe.
    assert!(Processor::new("open", Formula::state("door"))
        .evaluate(&system)
        .is_ok());
    assert!(Processor::new("open", Formula::state("nothing"))
        .evaluate(&system)
        .is_err());
}
