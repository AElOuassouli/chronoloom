//! One timeline per entity: the per-entity outputs of a processor, in batch.

#[path = "common/random.rs"]
mod common;
#[path = "common/update.rs"]
mod update;

use chronoloom::stream_system::Update;
use chronoloom::{
    Condition, Extent, Formula, Group, Key, Pattern, ProcessError, Processor, Recipe,
    SequenceSystem, StreamSystem, TimeIntervalSequence, TimePointEvent, TimePointSequence, Value,
};

use common::{bounds, extent, for_each_seed, span, spans, Random, DOMAIN_END, DOMAIN_START};

const CASES: u64 = 400;

fn crossing(agent: Value, direction: &str) -> Value {
    Value::record([("agent", agent), ("direction", Value::from(direction))])
}

fn direction(text: &'static str) -> Pattern {
    Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
}

/// Present from an "in" of an agent until the next "out" of the same agent.
fn present() -> Formula {
    Formula::between(direction("in"), direction("out"))
}

fn by_agent() -> Group {
    Group::new("gate", "agent")
}

fn gate(events: Vec<(i64, Value)>) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "gate",
        TimePointSequence::from_events(
            "gate",
            events
                .into_iter()
                .map(|(at, value)| TimePointEvent::new(at, value))
                .collect(),
        ),
    );

    system
}

fn spans_of(system: &StreamSystem, key: &str) -> Vec<(i64, i64)> {
    bounds(system.intervals(key).expect("a timeline"))
}

fn changes_of(system: &StreamSystem, key: &str) -> Vec<(i64, i64)> {
    system
        .points::<i64>(key)
        .expect("a count")
        .iter()
        .map(|point| (point.timestamp(), *point.value()))
        .collect()
}

/// The three outputs on one formula.
fn processor(formula: &Formula) -> Vec<Processor> {
    vec![
        Processor::new("presence", Recipe::per_entity(by_agent(), formula.clone())),
        Processor::new("anyone", Recipe::any_entity(by_agent(), formula.clone())),
        Processor::new("crowd", Recipe::count_entities(by_agent(), formula.clone())),
    ]
}

// ---------------------------------------------------------------------------
// Random streams
// ---------------------------------------------------------------------------

/// Random crossings of a few agents, a few of them without an agent field.
fn crossings(random: &mut Random) -> Vec<(i64, Value)> {
    (0..random.below(16))
        .map(|_| {
            let at = random.between(DOMAIN_START, DOMAIN_END);
            let direction = if random.chance(1, 2) { "in" } else { "out" };
            let value = if random.chance(1, 6) {
                Value::record([("direction", Value::from(direction))])
            } else {
                crossing(Value::from(random.between(0, 4)), direction)
            };
            (at, value)
        })
        .collect()
}

/// The events of one agent, as a member filtered by hand, and its first instant.
fn filtered(system: &SequenceSystem, agent: i64) -> (SequenceSystem, Option<i64>) {
    let records = system.points::<Value>("gate").expect("records");
    let events: Vec<TimePointEvent<Value>> = records
        .iter()
        .filter(|event| event.value().get("agent") == Some(&Value::from(agent)))
        .cloned()
        .collect();
    let first = events.first().map(TimePointEvent::timestamp);

    let mut hand = system.clone();
    hand.insert(
        "gate",
        TimePointSequence::from_events("gate", events).with_observed(records.observed().clone()),
    );

    (hand, first)
}

#[test]
fn an_entity_is_the_formula_over_its_own_records() {
    for_each_seed(CASES, |random, _| {
        let mut system = gate(crossings(random));
        let observed = extent(random);
        system.observe(&observed);
        let door = TimeIntervalSequence::from_spans("door", spans(random, 4))
            .with_observed(extent(random));
        system.insert("door", door);

        // The door is shared by every agent, the records are not.
        let formula = present().and(Formula::state("door"));
        let result = update::run(&system, &processor(&formula)).expect("a result");

        for agent in 0..4 {
            let (hand, first) = filtered(&system, agent);
            let key = format!("presence/{agent}");
            let Some(first) = first.filter(|&first| observed.contains(first)) else {
                assert!(!result.contains(&key), "{key} has no records");
                continue;
            };

            // An entity is observed from its first record on.
            let mut expected = formula.evaluate(&hand).expect("the formula");
            expected.observe(&Extent::from_spans(vec![span(first, i64::MAX)]));
            assert_eq!(result.intervals(&key), Some(&expected), "{key}");
        }
    });
}

#[test]
fn any_is_the_union_of_the_entities() {
    for_each_seed(CASES, |random, _| {
        let system = gate(crossings(random));
        let result = update::run(&system, &processor(&present())).expect("a result");

        let members: Vec<&TimeIntervalSequence> = result
            .keys()
            .filter(|key| key.as_str().starts_with("presence/"))
            .map(|key| result.intervals(key.as_str()).expect("a timeline"))
            .collect();
        let union = members.iter().fold(
            TimeIntervalSequence::new("union").with_observed(Extent::everything()),
            |union, member| union.union(member),
        );

        let anyone = result.intervals("anyone").expect("a timeline");
        assert_eq!(bounds(anyone), bounds(&union));
        // An entity that has not appeared is inactive, not unknown.
        assert!(anyone.is_observed(DOMAIN_START - 100));
    });
}

#[test]
fn the_count_is_the_number_of_active_entities_at_every_instant() {
    for_each_seed(CASES, |random, _| {
        let mut system = gate(crossings(random));
        system.observe(&Extent::from_spans(vec![span(
            DOMAIN_START,
            DOMAIN_END + 8,
        )]));
        let result = update::run(&system, &processor(&present())).expect("a result");

        let members: Vec<&TimeIntervalSequence> = result
            .keys()
            .filter(|key| key.as_str().starts_with("presence/"))
            .map(|key| result.intervals(key.as_str()).expect("a timeline"))
            .collect();
        let crowd = result.points::<i64>("crowd").expect("a count");

        for instant in DOMAIN_START..DOMAIN_END + 8 {
            let held = crowd
                .iter()
                .take_while(|point| point.timestamp() <= instant)
                .last()
                .map(|point| *point.value());
            let active = members
                .iter()
                .filter(|member| member.contains(instant))
                .count();
            assert_eq!(
                held,
                Some(i64::try_from(active).expect("few")),
                "at {instant}"
            );
        }

        // Canonical: no point repeats the count before it.
        let counts: Vec<i64> = crowd.iter().map(|point| *point.value()).collect();
        assert!(counts.windows(2).all(|pair| pair[0] != pair[1]));
    });
}

#[test]
fn the_durations_of_the_entities_add_up_to_the_count_times_its_duration() {
    for_each_seed(CASES, |random, _| {
        let mut system = gate(crossings(random));
        let horizon = DOMAIN_END + 8;
        system.observe(&Extent::from_spans(vec![span(0, horizon)]));
        let result = update::run(&system, &processor(&present())).expect("a result");

        let entities: u64 = result
            .keys()
            .filter(|key| key.as_str().starts_with("presence/"))
            .map(|key| {
                result
                    .intervals(key.as_str())
                    .expect("a timeline")
                    .active_duration()
            })
            .sum();

        // The count is known all over [0, horizon): each change holds to the next.
        let crowd = result.points::<i64>("crowd").expect("a count");
        let known = crowd.observed().as_slice();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].bounds(), (0, horizon));
        let changes: Vec<(i64, i64)> = changes_of(&result, "crowd");
        let counted: i64 = changes
            .iter()
            .enumerate()
            .map(|(index, (at, count))| {
                let until = changes.get(index + 1).map_or(horizon, |next| next.0);
                count * (until - at)
            })
            .sum();
        assert_eq!(i64::try_from(entities).expect("few"), counted);
    });
}

// ---------------------------------------------------------------------------
// What an entity is, and what it is called
// ---------------------------------------------------------------------------

fn visits(agent: Value, events: &[(i64, &str)]) -> Vec<(i64, Value)> {
    events
        .iter()
        .map(|(at, direction)| (*at, crossing(agent.clone(), direction)))
        .collect()
}

#[test]
fn members_are_filed_under_the_output_and_the_entity() {
    let system = gate(
        [
            visits(Value::from("agent-7"), &[(10, "in"), (20, "out")]),
            visits(Value::from(9), &[(12, "in"), (30, "out")]),
            visits(Value::from(1.5), &[(14, "in")]),
            visits(Value::from(true), &[(16, "in")]),
        ]
        .concat(),
    );
    let result = update::run(
        &system,
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");

    let keys: Vec<&str> = result.derived().map(Key::as_str).collect();
    assert_eq!(
        keys,
        [
            "presence/1.5",
            "presence/9",
            "presence/agent-7",
            "presence/true"
        ]
    );
    assert_eq!(spans_of(&result, "presence/agent-7"), [(10, 20)]);
    assert!(result.get("presence").is_none());
}

#[test]
fn a_slash_in_the_text_of_an_entity_is_kept() {
    let system = gate(visits(Value::from("zone/east"), &[(10, "in"), (20, "out")]));
    let result = update::run(
        &system,
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");

    assert_eq!(spans_of(&result, "presence/zone/east"), [(10, 20)]);
    assert_eq!(
        result.entity("presence/zone/east"),
        Some(&Value::from("zone/east"))
    );
}

#[test]
fn only_a_boolean_a_number_or_text_names_an_entity() {
    let records = vec![
        (10, Value::record([("direction", Value::from("in"))])),
        (11, crossing(Value::Null, "in")),
        (12, crossing(Value::record([("id", Value::from(1))]), "in")),
        (13, crossing(Value::from(f64::NAN), "in")),
        (14, crossing(Value::from(7), "in")),
        (15, Value::from(3)),
    ];
    let result = update::run(
        &gate(records),
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");

    let keys: Vec<&str> = result.derived().map(Key::as_str).collect();
    assert_eq!(keys, ["presence/7"]);
}

#[test]
fn records_of_other_entities_and_of_none_are_not_part_of_an_entity() {
    // Agent 7 enters, an unnamed record says "out", agent 9 leaves: neither
    // closes the presence of agent 7.
    let system = gate(vec![
        (10, crossing(Value::from(7), "in")),
        (12, Value::record([("direction", Value::from("out"))])),
        (14, crossing(Value::from(9), "out")),
        (20, crossing(Value::from(7), "out")),
    ]);
    let result = update::run(
        &system,
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");

    assert_eq!(spans_of(&result, "presence/7"), [(10, 20)]);
    // Agent 9 only ever left: nothing began.
    assert!(result.intervals("presence/9").expect("a member").is_empty());
}

#[test]
fn equal_numbers_are_one_entity_named_after_the_first() {
    let system = gate(vec![
        (10, crossing(Value::from(5), "in")),
        (15, crossing(Value::from(5.0), "out")),
        (20, crossing(Value::from(5.0), "in")),
    ]);
    let result = update::run(
        &system,
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");

    let keys: Vec<&str> = result.derived().map(Key::as_str).collect();
    assert_eq!(keys, ["presence/5"]);
    assert_eq!(result.intervals("presence/5").expect("a member").len(), 2);
}

#[test]
fn two_entities_with_one_name_are_refused() {
    // The text `5` and the integer `5` are different entities with one key.
    let system = gate(vec![
        (10, crossing(Value::from("5"), "in")),
        (11, crossing(Value::from(5), "in")),
    ]);
    let output = Recipe::per_entity(by_agent(), present());

    assert_eq!(
        update::run(&system, &[Processor::new("presence", output)]),
        Err(ProcessError::DuplicateMember {
            key: "presence/5".into()
        })
    );
}

#[test]
fn an_entity_cannot_take_the_key_of_another_output() {
    let system = gate(visits(Value::from(7), &[(10, "in")]));
    let processor = vec![
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("presence/7", Formula::state("nothing")),
    ];

    assert!(matches!(
        update::run(&system, &processor),
        Err(ProcessError::UnknownKey { .. } | ProcessError::DuplicateMember { .. })
    ));

    let processor = vec![
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("presence/7", Recipe::any_entity(by_agent(), present())),
    ];
    assert_eq!(
        update::run(&system, &processor),
        Err(ProcessError::DuplicateMember {
            key: "presence/7".into()
        })
    );
}

// ---------------------------------------------------------------------------
// Where an entity starts, and the extent of the aggregates
// ---------------------------------------------------------------------------

#[test]
fn an_entity_exists_from_its_first_record() {
    let system = gate(vec![
        (10, crossing(Value::from(7), "in")),
        (15, crossing(Value::from(7), "out")),
        (20, crossing(Value::from(9), "in")),
    ]);
    // Absent: observed and not present.
    let absent = !present();
    let result = update::run(&system, &processor(&absent)).expect("a result");

    // Agent 7 is absent from its leaving, and nothing is said before its first
    // record.
    let seven = result.intervals("presence/7").expect("a member");
    assert!(!seven.is_observed(9) && seven.is_observed(10));
    assert_eq!(bounds(seven), [(15, i64::MAX)]);
    assert!(!result
        .intervals("presence/9")
        .expect("a member")
        .is_observed(19));

    // An entity that has not appeared yet adds no uncertainty to the union:
    // known all along, active while agent 7 is absent.
    let anyone = result.intervals("anyone").expect("a timeline");
    assert!(anyone.is_observed(5) && !anyone.contains(5));
    assert!(anyone.contains(16));
}

#[test]
fn with_no_entity_nothing_is_active_and_the_count_is_zero() {
    let system = gate(vec![(
        10,
        Value::record([("direction", Value::from("in"))]),
    )]);
    let result = update::run(&system, &processor(&present())).expect("a result");

    assert_eq!(result.derived().count(), 2);
    assert!(result.intervals("anyone").expect("a timeline").is_empty());
    assert_eq!(changes_of(&result, "crowd"), [(i64::MIN, 0)]);
}

#[test]
fn a_hand_over_at_one_instant_counts_once() {
    let system = gate(vec![
        (10, crossing(Value::from(1), "in")),
        (20, crossing(Value::from(1), "out")),
        (20, crossing(Value::from(2), "in")),
        (30, crossing(Value::from(2), "out")),
    ]);
    let result = update::run(&system, &processor(&present())).expect("a result");

    // No point at 20: one agent leaves as another enters.
    assert_eq!(
        changes_of(&result, "crowd"),
        [(i64::MIN, 0), (10, 1), (30, 0)]
    );
    assert_eq!(spans_of(&result, "anyone"), [(10, 30)]);
}

#[test]
fn the_count_is_known_where_every_entity_is() {
    // Agent 2's timeline is known only while the door was observed.
    let mut system = gate(vec![
        (10, crossing(Value::from(1), "in")),
        (30, crossing(Value::from(2), "in")),
    ]);
    system.insert(
        "door",
        TimeIntervalSequence::from_spans("door", vec![span(0, 100)])
            .with_observed(Extent::from_spans(vec![span(0, 40), span(60, 100)])),
    );
    let result = update::run(
        &system,
        &[Processor::new(
            "crowd",
            Recipe::count_entities(by_agent(), present().and(Formula::state("door"))),
        )],
    )
    .expect("a result");

    let crowd = result.points::<i64>("crowd").expect("a count");
    // Two runs, each opening with a point; the gap [40, 60) has none. Before
    // the door was seen, a presence is known inactive.
    let runs: Vec<(i64, i64)> = crowd
        .observed()
        .as_slice()
        .iter()
        .map(|run| run.bounds())
        .collect();
    assert_eq!(runs, [(i64::MIN, 40), (60, 100)]);
    assert_eq!(
        changes_of(&result, "crowd"),
        [(i64::MIN, 0), (10, 1), (30, 2), (60, 2)]
    );
}

#[test]
fn an_entity_not_seen_yet_adds_no_uncertainty() {
    // The records were not observed over [20, 30), and the door is active
    // there. Agent 1's timeline (present or door) is known over the gap
    // because the door is active; agent 2, who only appears at 70, must not
    // make the result unknown there.
    let mut system = SequenceSystem::new();
    system.insert(
        "gate",
        TimePointSequence::from_events(
            "gate",
            vec![
                TimePointEvent::new(10, crossing(Value::from(1), "in")),
                TimePointEvent::new(70, crossing(Value::from(2), "in")),
            ],
        )
        .with_observed(Extent::from_spans(vec![span(0, 20), span(30, 100)])),
    );
    system.insert(
        "door",
        TimeIntervalSequence::from_spans("door", vec![span(20, 30)]),
    );

    let formula = present().or(Formula::state("door"));
    let result = update::run(
        &system,
        &[
            Processor::new("crowd", Recipe::count_entities(by_agent(), formula.clone())),
            Processor::new("anyone", Recipe::any_entity(by_agent(), formula)),
        ],
    )
    .expect("a result");

    let runs = |observed: &Extent| -> Vec<(i64, i64)> {
        observed.as_slice().iter().map(|run| run.bounds()).collect()
    };
    let crowd = result.points::<i64>("crowd").expect("a count");
    let anyone = result.intervals("anyone").expect("a timeline");

    // Known over all the records, the gap included.
    assert_eq!(runs(crowd.observed()), [(0, 100)]);
    assert_eq!(runs(anyone.observed()), [(0, 100)]);
    assert_eq!(
        changes_of(&result, "crowd"),
        [(0, 0), (10, 1), (30, 0), (70, 1)]
    );
}

// ---------------------------------------------------------------------------
// Reading the members, and the order of evaluation
// ---------------------------------------------------------------------------

fn two_agents() -> SequenceSystem {
    gate(vec![
        (10, crossing(Value::from(7), "in")),
        (12, crossing(Value::from(9), "in")),
        (20, crossing(Value::from(7), "out")),
        (30, crossing(Value::from(9), "out")),
    ])
}

#[test]
fn another_formula_may_read_a_member_of_a_per_entity_output() {
    // Added before the output it reads: the order does not matter.
    let processor = vec![
        Processor::new("seven_again", Formula::state("presence/7").shift(0, 5)),
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
    ];
    let result = update::run(&two_agents(), &processor).expect("a result");

    assert_eq!(spans_of(&result, "seven_again"), [(10, 25)]);
    assert!(processor.iter().all(|processor| processor.check().is_ok()));
}

#[test]
fn a_member_of_an_entity_that_does_not_exist_is_unknown() {
    let processor = vec![
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("ghost", Formula::state("presence/12")),
    ];

    assert_eq!(
        update::run(&two_agents(), &processor),
        Err(ProcessError::UnknownKey {
            key: "presence/12".into()
        })
    );
}

#[test]
fn a_per_entity_formula_that_reads_its_own_members_is_a_cycle() {
    let processor = [Processor::new(
        "presence",
        Recipe::per_entity(by_agent(), present().or(Formula::state("presence/7"))),
    )];

    assert_eq!(
        processor[0].check(),
        Err(ProcessError::Cycle {
            keys: vec!["presence".into()]
        })
    );
}

#[test]
fn any_and_count_are_read_by_their_own_key_like_any_output() {
    let processor = vec![
        Processor::new(
            "alone",
            Formula::while_matching(Pattern::new("crowd", Condition::Equals(1_i64)))
                .and(Formula::state("anyone")),
        ),
        Processor::new("anyone", Recipe::any_entity(by_agent(), present())),
        Processor::new("crowd", Recipe::count_entities(by_agent(), present())),
    ];
    let result = update::run(&two_agents(), &processor).expect("a result");

    // Agent 7 alone from 10 to 12, agent 9 alone from 20 to 30.
    assert_eq!(spans_of(&result, "alone"), [(10, 12), (20, 30)]);
}

#[test]
fn the_group_reads_an_output_of_the_same_processor() {
    // The records of a group may be an output of the processor.
    let processor = vec![
        Processor::new(
            "entered",
            chronoloom::Events::Matching(Pattern::field(
                "gate",
                "direction",
                Condition::Equals(Value::from("in")),
            )),
        ),
        Processor::new(
            "arrivals",
            Recipe::per_entity(
                Group::new("entered", "agent"),
                Formula::occurs(Pattern::field("entered", "direction", Condition::Any)),
            ),
        ),
    ];
    let result = update::run(&two_agents(), &processor).expect("a result");

    assert_eq!(spans_of(&result, "arrivals/7"), [(10, 11)]);
    assert_eq!(spans_of(&result, "arrivals/9"), [(12, 13)]);
}

// ---------------------------------------------------------------------------
// Lineage, applying, and errors
// ---------------------------------------------------------------------------

#[test]
fn each_member_knows_its_processor_and_its_entity() {
    let output = Recipe::per_entity(by_agent(), present());
    let system = update::run(&two_agents(), &[Processor::new("presence", output.clone())])
        .expect("a result");

    assert_eq!(
        system.producer("presence/7").map(Processor::recipe),
        Some(&output)
    );
    assert_eq!(system.entity("presence/9"), Some(&Value::from(9)));
    assert_eq!(system.entity("gate"), None);
    assert!(system.is_derived("presence/7") && !system.is_derived("gate"));
}

#[test]
fn an_update_derives_the_members_again_from_the_current_records() {
    let mut system = update::run(
        &two_agents(),
        &[
            Processor::new("presence", Recipe::per_entity(by_agent(), present())),
            Processor::new("anyone", Recipe::any_entity(by_agent(), present())),
        ],
    )
    .expect("a result");
    assert_eq!(spans_of(&system, "presence/7"), [(10, 20)]);

    let mut update = Update::new();
    update.points(
        "gate",
        [TimePointEvent::new(40, crossing(Value::from(7), "in"))],
    );
    let report = system.update(update).expect("recorded");

    assert_eq!(spans_of(&system, "presence/7")[0], (10, 20));
    assert_eq!(spans_of(&system, "presence/7").len(), 2);
    // The other member did not change, and is still a member of the output.
    assert!(report.changed().contains("presence/7") && !report.changed().contains("presence/9"));
    assert_eq!(spans_of(&system, "presence/9"), [(12, 30)]);
    assert_eq!(system.entity("presence/7"), Some(&Value::from(7)));
}

#[test]
fn a_processor_that_cannot_be_evaluated_changes_nothing() {
    let mut system = update::run(
        &two_agents(),
        &[Processor::new(
            "presence",
            Recipe::per_entity(by_agent(), present()),
        )],
    )
    .expect("a result");
    let before = system.clone();

    assert!(system
        .add_processor(Processor::new("broken", Formula::state("nothing")))
        .is_err());
    assert_eq!(system, before);
}

#[test]
fn a_per_entity_output_has_no_single_sequence() {
    let output = Recipe::per_entity(by_agent(), present());
    assert_eq!(
        output.evaluate(&two_agents()),
        Err(ProcessError::SeveralMembers)
    );
    assert_eq!(
        output.to_string(),
        r#"between(gate.direction = "in", gate.direction = "out") per gate.agent"#
    );

    // The others do have one.
    let any = Recipe::any_entity(by_agent(), present());
    let sequence = any.evaluate(&two_agents()).expect("a timeline");
    assert_eq!(
        sequence.as_time_interval().map(TimeIntervalSequence::len),
        Some(1)
    );
    assert!(any.to_string().starts_with("any("));
    assert!(Recipe::count_entities(by_agent(), present())
        .to_string()
        .starts_with("count("));
}

#[test]
fn entity_recipes_evaluate_to_one_labelled_sequence() {
    let system = two_agents();
    let recipe = Recipe::count_entities(by_agent(), present());
    let sequence = recipe.evaluate(&system).expect("a sequence");
    let counts = sequence.as_time_point::<i64>().expect("integer points");

    assert_eq!(counts.label(), recipe.to_string());
    let any = Recipe::any_entity(by_agent(), present());
    let evaluated = any.evaluate(&system).expect("a timeline");
    let timeline = evaluated.as_time_interval().expect("a timeline");
    assert_eq!(timeline.label(), any.to_string());
}

#[test]
fn the_records_of_a_group_must_be_values() {
    let mut system = SequenceSystem::new();
    system.insert("numbers", TimePointSequence::<i64>::new("numbers"));
    system.insert("timeline", TimeIntervalSequence::new("timeline"));
    let output = |key: &str| Recipe::count_entities(Group::new(key, "agent"), present());

    assert!(matches!(
        output("numbers").evaluate(&system),
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
    assert!(matches!(
        output("timeline").evaluate(&system),
        Err(ProcessError::NotPoints { .. })
    ));
    assert!(matches!(
        output("nothing").evaluate(&system),
        Err(ProcessError::UnknownKey { .. })
    ));
}
