//! One timeline per entity, online: entities appear on first sight, and what is
//! emitted is what the batch evaluation gives.

#[path = "common/update.rs"]
mod update;

use chronoloom::{
    Condition, Edge, Emission, Emitted, Extent, Formula, Group, LiveProcessor, Pattern,
    ProcessError, Processor, Recipe, SequenceSystem, TimeIntervalEvent, TimePointEvent,
    TimePointSequence, Value,
};

fn crossing(agent: Value, direction: &str) -> Value {
    Value::record([("agent", agent), ("direction", Value::from(direction))])
}

fn direction(text: &'static str) -> Pattern {
    Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
}

fn present() -> Formula {
    Formula::between(direction("in"), direction("out"))
}

fn by_agent() -> Group {
    Group::new("gate", "agent")
}

/// Presence per agent, whether anyone is present, and how many.
fn presence() -> Vec<Processor> {
    vec![
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("anyone", Recipe::any_entity(by_agent(), present())),
        Processor::new("crowd", Recipe::count_entities(by_agent(), present())),
    ]
}

/// What a key emitted, as `(instant, edge or count)`: `0` for a start and `1`
/// for an end of a state, the count for a point.
fn emitted(emissions: &[Emission], key: &str) -> Vec<(i64, i64)> {
    emissions
        .iter()
        .filter(|emission| emission.key.as_str() == key)
        .map(|emission| match &emission.item {
            Emitted::Edge(edge) => (edge.timestamp(), i64::from(*edge.value() == Edge::End)),
            Emitted::Point(point) => (
                point.timestamp(),
                *point.value().downcast_ref::<i64>().expect("a count"),
            ),
        })
        .collect()
}

#[test]
fn entities_appear_on_first_sight_and_emissions_carry_the_member_key() {
    let mut live = LiveProcessor::new(&presence(), 0).expect("valid");
    assert_eq!(live.frontier("presence/agent-7"), None);

    live.on_point(
        "gate",
        TimePointEvent::new(10, crossing(Value::from("agent-7"), "in")),
    )
    .expect("on time");
    assert_eq!(live.frontier("presence/agent-7"), Some(i64::MIN));
    assert_eq!(live.frontier("presence/agent-9"), None);

    let first = live.advance("gate", 15).expect("known");
    assert_eq!(emitted(&first, "presence/agent-7"), [(10, 0)]);
    assert_eq!(emitted(&first, "anyone"), [(10, 0)]);
    // The count opens with its first point, then changes at 10.
    assert_eq!(emitted(&first, "crowd"), [(i64::MIN, 0), (10, 1)]);

    live.on_point(
        "gate",
        TimePointEvent::new(18, crossing(Value::from("agent-9"), "in")),
    )
    .expect("on time");
    live.on_point(
        "gate",
        TimePointEvent::new(20, crossing(Value::from("agent-7"), "out")),
    )
    .expect("on time");
    let second = live.advance("gate", 25).expect("known");
    assert_eq!(emitted(&second, "presence/agent-7"), [(20, 1)]);
    assert_eq!(emitted(&second, "presence/agent-9"), [(18, 0)]);
    assert_eq!(emitted(&second, "crowd"), [(18, 2), (20, 1)]);

    // The output has no member of its own, and its frontier is that of an
    // entity not seen yet.
    assert_eq!(live.frontier("presence"), Some(25));
    assert_eq!(live.frontier("presence/agent-9"), Some(25));
}

#[test]
fn the_emissions_are_the_batch_result() {
    let events = [
        (10, "a", "in"),
        (12, "b", "in"),
        (20, "a", "out"),
        (20, "c", "in"),
        (31, "b", "out"),
        (34, "c", "out"),
        (40, "a", "in"),
    ];

    let mut live = LiveProcessor::new(&presence(), 0).expect("valid");
    let mut all = Vec::new();
    for (index, (at, agent, way)) in events.into_iter().enumerate() {
        live.on_point(
            "gate",
            TimePointEvent::new(at, crossing(Value::from(agent), way)),
        )
        .expect("on time");
        if index % 2 == 1 {
            all.extend(live.advance("gate", at).expect("known"));
        }
    }
    all.extend(live.finish(50).expect("done"));

    let mut system = SequenceSystem::new();
    system.insert(
        "gate",
        TimePointSequence::from_events(
            "gate",
            events
                .into_iter()
                .map(|(at, agent, way)| TimePointEvent::new(at, crossing(Value::from(agent), way)))
                .collect(),
        )
        .with_observed(Extent::from_spans(vec![TimeIntervalEvent::span(
            i64::MIN,
            50,
        )
        .expect("ordered")])),
    );
    let batch = update::run(&update::streams_of(&system), &presence()).expect("a result");

    for agent in ["a", "b", "c"] {
        let key = format!("presence/{agent}");
        let expected: Vec<(i64, i64)> = batch
            .intervals(&key)
            .expect("a member")
            .iter()
            .flat_map(|span| [(span.start(), 0), (span.end(), 1)])
            .collect();
        assert_eq!(emitted(&all, &key), expected, "{key}");
    }
    let anyone: Vec<(i64, i64)> = batch
        .intervals("anyone")
        .expect("a timeline")
        .iter()
        .flat_map(|span| [(span.start(), 0), (span.end(), 1)])
        .collect();
    assert_eq!(emitted(&all, "anyone"), anyone);
    let crowd: Vec<(i64, i64)> = batch
        .points::<i64>("crowd")
        .expect("a count")
        .iter()
        .map(|point| (point.timestamp(), *point.value()))
        .collect();
    assert_eq!(emitted(&all, "crowd"), crowd);
}

#[test]
fn another_output_reads_the_union_and_the_count_online() {
    let mut processors = presence();
    processors.push(Processor::new(
        "alone",
        Formula::while_matching(Pattern::new("crowd", Condition::Equals(1_i64)))
            .and(Formula::state("anyone")),
    ));
    let mut live = LiveProcessor::new(&processors, 0).expect("valid");

    live.on_point(
        "gate",
        TimePointEvent::new(10, crossing(Value::from(7), "in")),
    )
    .expect("on time");
    live.on_point(
        "gate",
        TimePointEvent::new(12, crossing(Value::from(9), "in")),
    )
    .expect("on time");
    live.on_point(
        "gate",
        TimePointEvent::new(20, crossing(Value::from(7), "out")),
    )
    .expect("on time");
    let mut all = live.advance("gate", 25).expect("known");
    all.extend(live.finish(40).expect("done"));

    // Alone from 10 to 12, and again from 20 to the end.
    assert_eq!(emitted(&all, "alone"), [(10, 0), (12, 1), (20, 0), (40, 1)]);
}

#[test]
fn a_live_processor_cannot_read_the_members_of_a_per_entity_output() {
    let processors = [
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("seven", Formula::state("presence/7")),
    ];
    assert_eq!(
        LiveProcessor::new(&processors, 0).err(),
        Some(ProcessError::GroupedRead {
            key: "presence/7".into()
        })
    );

    // The records a group reads are an input, not an output.
    let chained = [
        Processor::new("changes", chronoloom::Events::changes("gate")),
        Processor::new(
            "presence",
            Recipe::per_entity(Group::new("changes", "agent"), present()),
        ),
    ];
    assert_eq!(
        LiveProcessor::new(&chained, 0).err(),
        Some(ProcessError::GroupedRead {
            key: "changes".into()
        })
    );
}

#[test]
fn two_entities_with_one_name_are_refused_and_nothing_is_recorded() {
    let mut live = LiveProcessor::new(&presence(), 0).expect("valid");
    live.on_point(
        "gate",
        TimePointEvent::new(10, crossing(Value::from("5"), "in")),
    )
    .expect("on time");
    let held = live.retained();

    assert_eq!(
        live.on_point(
            "gate",
            TimePointEvent::new(11, crossing(Value::from(5), "in"))
        ),
        Err(ProcessError::DuplicateMember {
            key: "presence/5".into()
        })
    );
    assert_eq!(live.retained(), held);

    // The entity that was there is still served.
    live.on_point(
        "gate",
        TimePointEvent::new(12, crossing(Value::from("5"), "out")),
    )
    .expect("on time");
    let emitted_all = live.finish(20).expect("done");
    assert_eq!(emitted(&emitted_all, "presence/5"), [(10, 0), (12, 1)]);
}

#[test]
fn an_entity_cannot_take_the_key_of_an_output_online() {
    let processors = [
        Processor::new("presence", Recipe::per_entity(by_agent(), present())),
        Processor::new("presence/7", Formula::state("door")),
    ];
    let mut live = LiveProcessor::new(&processors, 0).expect("valid");

    assert_eq!(
        live.on_point(
            "gate",
            TimePointEvent::new(10, crossing(Value::from(7), "in"))
        ),
        Err(ProcessError::DuplicateMember {
            key: "presence/7".into()
        })
    );
}

#[test]
fn the_state_grows_with_the_entities_seen_and_not_with_the_stream() {
    fn run(agents: i64, ticks: i64) -> (usize, LiveProcessor) {
        let mut live = LiveProcessor::new(&presence(), 0).expect("valid");
        let mut worst = 0;
        for tick in 0..ticks {
            // Each agent comes and goes on its own turn.
            let agent = tick % agents;
            let way = if (tick / agents) % 2 == 0 {
                "in"
            } else {
                "out"
            };
            live.on_point(
                "gate",
                TimePointEvent::new(tick, crossing(Value::from(agent), way)),
            )
            .expect("on time");
            live.advance("gate", tick + 1).expect("known");
            worst = worst.max(live.retained());
        }

        (worst, live)
    }

    let (short, _) = run(5, 100);
    let (long, live) = run(5, 400);
    // Ten times the stream holds the same.
    assert_eq!(short, long, "the window does not grow with the stream");
    assert!(long < 100, "retained {long}");

    // More entities hold more, in proportion.
    let (many, _) = run(20, 400);
    assert!(
        many > long && many < 4 * long + 40,
        "retained {many} against {long}"
    );
    drop(live);
}
