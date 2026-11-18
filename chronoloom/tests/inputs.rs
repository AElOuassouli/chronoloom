//! Declared inputs and `Processor::validate`: every problem of a processor
//! listed in a stable order, and a system or a stream held to the declarations.

use chronoloom::{
    Condition, Events, Formula, Group, Input, Kind, Output, Pattern, ProcessError, Processor,
    Schema, SequenceSystem, TimeIntervalEvent, TimeIntervalSequence, TimePointEvent,
    TimePointSequence, Value,
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

fn declared() -> Processor {
    Processor::new()
        .with_input("temperature", Input::Points(Kind::Float))
        .with_input("door", Input::Timeline)
        .with_input("records", Input::Records(schema()))
}

#[test]
fn a_valid_processor_validates_with_and_without_declarations() {
    let processor = declared()
        .with(
            "hot",
            Formula::while_matching(Pattern::new("temperature", Condition::Greater(5.0))),
        )
        .with("open", Formula::state("door").or(Formula::state("hot")))
        .with(
            "busy",
            Formula::while_matching(Pattern::field(
                "records",
                "site.zone",
                Condition::Equals(Value::from("gate")),
            )),
        )
        .with("counts", Events::project("records", "count", Kind::Float))
        .with(
            "each",
            Output::per_entity(Group::new("records", "agent"), Formula::state("door")),
        )
        .with("later", Formula::state("each/agent-7"));

    assert_eq!(processor.validate(), Ok(()));
    assert_eq!(Processor::new().validate(), Ok(()));

    // Without declarations, nothing is known about the keys that are read.
    let bare = Processor::new().with("a", Formula::state("anything"));
    assert_eq!(bare.validate(), Ok(()));
}

#[test]
fn validate_lists_every_problem_in_output_then_formula_order() {
    let processor = declared()
        .with_input("zeta", Input::Timeline)
        .with_input("shadowed", Input::Timeline)
        .with("shadowed", Formula::state("door"))
        .with(
            // Two problems in one formula, left to right.
            "b",
            Formula::state("temperature").and(Formula::state("missing")),
        )
        .with(
            "a",
            Formula::while_matching(Pattern::new("door", Condition::Equals(1_i64))).or(
                Formula::occurs(Pattern::new("temperature", Condition::Any::<i64>)),
            ),
        )
        .with(
            "c",
            Output::per_entity(
                Group::new("records", "nobody"),
                Formula::while_matching(Pattern::field(
                    "records",
                    "agent",
                    Condition::Equals(Value::Integer(3)),
                )),
            ),
        )
        .with("d", Events::project("records", "agent", Kind::Integer))
        .with("e", Events::project("records", "site.street", Kind::Text))
        .with("f", Events::project("records", "count", Kind::Float));

    let float = std::any::type_name::<f64>();
    let integer = std::any::type_name::<i64>();
    assert_eq!(
        processor.validate(),
        Err(vec![
            ProcessError::InputIsOutput {
                key: "shadowed".into()
            },
            in_output("a", ProcessError::NotPoints { key: "door".into() }),
            in_output(
                "a",
                ProcessError::ValueTypeMismatch {
                    key: "temperature".into(),
                    expected: float,
                    found: integer,
                }
            ),
            in_output(
                "b",
                ProcessError::NotATimeline {
                    key: "temperature".into()
                }
            ),
            in_output(
                "b",
                ProcessError::UnknownKey {
                    key: "missing".into()
                }
            ),
            in_output(
                "c",
                ProcessError::MissingField {
                    key: "records".into(),
                    path: "nobody".into()
                }
            ),
            in_output(
                "c",
                ProcessError::FieldMismatch {
                    key: "records".into(),
                    path: "agent".into(),
                    expected: "text".into(),
                    found: "integer",
                }
            ),
            in_output(
                "d",
                ProcessError::ProjectionMismatch {
                    key: "records".into(),
                    path: "agent".into(),
                    expected: Kind::Integer,
                    found: "text",
                }
            ),
            in_output(
                "e",
                ProcessError::MissingField {
                    key: "records".into(),
                    path: "site.street".into()
                }
            ),
        ])
    );
}

#[test]
fn validate_agrees_with_check_on_a_cycle() {
    let processor = declared()
        .with("a", Formula::state("b"))
        .with("b", Formula::state("a"))
        .with("c", Formula::state("nowhere"));

    let problems = processor.validate().unwrap_err();
    assert_eq!(Err(problems[0].clone()), processor.check());
    assert!(matches!(problems[0], ProcessError::Cycle { .. }));
    assert_eq!(problems.len(), 2, "{problems:?}");
}

#[test]
fn a_path_through_an_any_shape_is_not_missing() {
    let processor = Processor::new()
        .with_input(
            "loose",
            Input::Records(Schema::record([("data", Schema::Any)])),
        )
        .with(
            "a",
            Formula::occurs(Pattern::field("loose", "data.deep.er", Condition::Any)),
        );

    assert_eq!(processor.validate(), Ok(()));
}

fn world() -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "temperature",
        TimePointSequence::from_events("temperature", vec![TimePointEvent::new(1, 2.0_f64)]),
    );
    system.insert(
        "door",
        TimeIntervalSequence::from_spans("door", vec![TimeIntervalEvent::span(0, 5).unwrap()]),
    );
    system.insert(
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
    );
    system
}

#[test]
fn run_and_apply_hold_the_system_to_the_declarations() {
    let processor = declared().with("open", Formula::state("door"));
    assert!(processor.run(&world()).is_ok());

    let mut missing = world();
    missing.remove("door");
    assert_eq!(
        processor.run(&missing),
        Err(ProcessError::UnknownKey { key: "door".into() })
    );

    let mut flipped = world();
    flipped.insert("door", TimePointSequence::<f64>::new("door"));
    assert_eq!(
        processor.run(&flipped),
        Err(ProcessError::NotATimeline { key: "door".into() })
    );

    let mut flipped = world();
    flipped.insert("temperature", TimeIntervalSequence::new("temperature"));
    assert_eq!(
        processor.run(&flipped),
        Err(ProcessError::NotPoints {
            key: "temperature".into()
        })
    );

    let mut integers = world();
    integers.insert("temperature", TimePointSequence::<i64>::new("temperature"));
    assert!(matches!(
        processor.run(&integers),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "temperature"
    ));

    let mut stray = world();
    stray.insert(
        "records",
        TimePointSequence::from_events(
            "records",
            vec![TimePointEvent::new(
                2,
                Value::record([("agent", Value::from(1))]),
            )],
        ),
    );
    assert!(matches!(
        processor.run(&stray),
        Err(ProcessError::SchemaMismatch { key, .. }) if key.as_str() == "records"
    ));

    // Nothing is inserted when the system does not satisfy a declaration.
    let mut system = missing;
    assert!(processor.apply(&mut system).is_err());
    assert!(!system.contains("open"));
}

#[test]
fn a_processor_without_declarations_behaves_as_before() {
    let processor = Processor::new().with("open", Formula::state("door"));
    let mut system = SequenceSystem::new();
    system.insert("door", TimeIntervalSequence::new("door"));

    assert!(processor.run(&system).is_ok());
    assert_eq!(
        processor.run(&SequenceSystem::new()),
        Err(ProcessError::UnknownKey { key: "door".into() })
    );
}

#[test]
fn live_refuses_reads_that_contradict_the_declarations() {
    let contradicting = declared().with(
        "a",
        Formula::while_matching(Pattern::new("temperature", Condition::Greater(5_i64))),
    );
    assert!(matches!(
        contradicting.live(0),
        Err(ProcessError::ValueTypeMismatch { key, .. }) if key.as_str() == "temperature"
    ));

    let shape = declared().with("a", Formula::state("temperature"));
    assert_eq!(
        shape.live(0).err(),
        Some(ProcessError::NotATimeline {
            key: "temperature".into()
        })
    );

    let shadowed = declared().with("door", Formula::state("temperature"));
    assert_eq!(
        shadowed.live(0).err(),
        Some(ProcessError::InputIsOutput { key: "door".into() })
    );
}

#[test]
fn live_creates_the_declared_inputs_and_refuses_records_off_the_schema() {
    let processor = declared().with("changes", Events::changes("temperature"));
    let mut live = processor.live(0).unwrap();

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
