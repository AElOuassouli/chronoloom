//! Checkpoints of a live processor, behind the `serde` feature: a processor
//! saved at any point and restored goes on exactly as one that never stopped,
//! and a document that contradicts itself is refused.
#![cfg(feature = "serde")]

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;
#[path = "common/update.rs"]
mod update;

use serde_json::{json, Value as Json};

use chronoloom::{
    Condition, Edge, Emission, Emitted, Events, Formula, Group, Input, Kind, LiveProcessor,
    Pattern, ProcessError, Processor, Recipe, Schema, SequenceSystem, StreamSystem, TimePointEvent,
    TimePointSequence, Value,
};

use update::Evaluate;

use common::{bounds, for_each_seed, span, Random};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

const CASES: u64 = 400;

/// The checkpoint of a live processor, as text.
fn save(live: &LiveProcessor) -> String {
    serde_json::to_string(live).expect("a checkpoint is written")
}

/// Restore a checkpoint, and check that saving the result gives the same text.
fn restore(text: &str) -> LiveProcessor {
    let live: LiveProcessor = serde_json::from_str(text).expect("a checkpoint loads");
    assert_eq!(save(&live), text, "a restored processor saves what it was");
    live
}

fn refusal(document: &Json) -> String {
    match serde_json::from_value::<LiveProcessor>(document.clone()) {
        Ok(_) => panic!("the document should be refused: {document}"),
        Err(error) => error.to_string(),
    }
}

/// What a host sees of one call.
#[derive(Debug, PartialEq)]
struct Seen {
    emitted: Vec<Emission>,
    frontiers: Vec<Option<i64>>,
    retained: usize,
}

fn apply(live: &mut LiveProcessor, op: &Op) -> Vec<Emission> {
    let outcome = match op {
        Op::Point(key, at, value) => live
            .on_point(key, TimePointEvent::new(*at, *value))
            .map(|()| Vec::new()),
        Op::Record(key, at, value) => live
            .on_point(key, TimePointEvent::new(*at, value.clone()))
            .map(|()| Vec::new()),
        Op::Span(key, start, end) => live.on_span(key, span(*start, *end)).map(|()| Vec::new()),
        Op::Advance(key, to) => live.advance(key, *to),
    };
    match outcome {
        Ok(emitted) => emitted,
        // An input that no output reads is not one of the processor's.
        Err(ProcessError::UnknownKey { .. }) => Vec::new(),
        Err(error) => panic!("{op:?}: {error}"),
    }
}

fn seen(live: &LiveProcessor, processor: &[Processor], emitted: Vec<Emission>) -> Seen {
    Seen {
        emitted,
        frontiers: processor
            .iter()
            .map(Processor::output)
            .map(|key| live.frontier(key.as_str()))
            .collect(),
        retained: live.retained(),
    }
}

/// Run the stream, saving and restoring before the operations at `stops`, then
/// finish; every call is observed.
fn run(processor: &[Processor], stream: &Stream, stops: &[usize]) -> Vec<Seen> {
    let mut live = LiveProcessor::new(processor, stream.from).expect("a valid processor");
    let mut observed = Vec::new();

    for (index, op) in stream.ops.iter().enumerate() {
        for _ in stops.iter().filter(|stop| **stop == index) {
            live = restore(&save(&live));
        }
        let emitted = apply(&mut live, op);
        observed.push(seen(&live, processor, emitted));
    }
    for _ in stops.iter().filter(|stop| **stop >= stream.ops.len()) {
        live = restore(&save(&live));
    }
    let emitted = live.finish(stream.horizon).expect("finishing cannot fail");
    observed.push(seen(&live, processor, emitted));

    observed
}

fn stop(random: &mut Random, stream: &Stream) -> usize {
    random.below(stream.ops.len() as u64 + 1) as usize
}

#[test]
fn a_restored_processor_goes_on_as_one_that_never_stopped() {
    for_each_seed(CASES, |random, _| {
        let processor = processors(random);
        let stream = stream(random);
        let uninterrupted = run(&processor, &stream, &[]);

        // Once, at a random point; right after `live`; and twice in one run,
        // the second time possibly right after the first.
        let first = stop(random, &stream);
        let second = stop(random, &stream);
        for stops in [vec![first], vec![0], vec![first, second]] {
            assert_eq!(
                run(&processor, &stream, &stops),
                uninterrupted,
                "checkpoints before the operations {stops:?} of {:?}",
                stream.ops
            );
        }
    });
}

#[test]
fn a_checkpoint_after_finish_stays_finished() {
    for_each_seed(100, |random, _| {
        let processor = processors(random);
        let stream = stream(random);
        let mut live = LiveProcessor::new(&processor, stream.from).expect("valid");
        for op in &stream.ops {
            apply(&mut live, op);
        }
        live.finish(stream.horizon).expect("finishing");

        let mut restored = restore(&save(&live));
        assert_eq!(restored.finish(stream.horizon), Ok(Vec::new()));
        assert_eq!(restored.retained(), 0);
        assert_eq!(restored.processors(), &processor);
        for key in processor.iter().map(Processor::output) {
            assert_eq!(restored.frontier(key.as_str()), Some(i64::MAX));
        }
        for key in POINT_INPUTS
            .iter()
            .chain(&STATE_INPUTS)
            .chain(&RECORD_INPUTS)
        {
            let late = restored.advance(key, i64::MAX);
            assert!(matches!(late, Err(ProcessError::Finished)));
        }
    });
}

fn busy() -> Vec<Processor> {
    vec![Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64))),
    )]
}

#[test]
fn a_host_that_restarts_gets_the_batch_result() {
    let processor = busy();
    let readings = [(3, 2_i64), (7, 9), (25, 1), (40, 8)];

    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    let mut edges = Vec::new();
    for (index, (at, value)) in readings.into_iter().enumerate() {
        live.on_point("count", TimePointEvent::new(at, value))
            .expect("on time");
        edges.extend(live.advance("count", at + 1).expect("known"));
        if index == 1 {
            // The process dies here, and a new one starts from the checkpoint.
            live = restore(&save(&live));
        }
    }
    edges.extend(live.finish(50).expect("finishing"));

    let mut recorded = SequenceSystem::new();
    recorded.insert(
        "count",
        TimePointSequence::from_events(
            "count",
            readings
                .into_iter()
                .map(|(at, value)| TimePointEvent::new(at, value))
                .collect(),
        )
        .with_observed(chronoloom::Extent::from_spans(vec![
            chronoloom::TimeIntervalEvent::span(i64::MIN, 50).expect("ordered"),
        ])),
    );
    let batch = update::run(&update::streams_of(&recorded), &processor).expect("the batch result");

    let mut rebuilt = StreamSystem::new();
    rebuilt
        .insert_stream(
            "edges",
            TimePointSequence::from_events(
                "edges",
                edges
                    .iter()
                    .filter_map(|emission| match &emission.item {
                        Emitted::Edge(edge) => Some(*edge),
                        Emitted::Point(_) => None,
                    })
                    .collect(),
            ),
        )
        .unwrap();
    let timeline = Formula::between(
        Pattern::new("edges", Condition::Equals(Edge::Start)),
        Pattern::new("edges", Condition::Equals(Edge::End)),
    )
    .evaluate_over(&rebuilt)
    .expect("edges are edges");
    assert_eq!(
        bounds(&timeline),
        bounds(batch.intervals("busy").expect("a timeline"))
    );
}

#[test]
fn the_checkpoint_holds_its_processor() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.on_point("count", TimePointEvent::new(7, 9_i64))
        .expect("on time");
    live.advance("count", 10).expect("known");

    let restored = restore(&save(&live));
    assert_eq!(restored.processors(), live.processors());
    assert_eq!(restored.processors(), &busy());
    assert_eq!(restored.frontier("busy"), Some(10));
}

#[test]
fn an_input_without_a_type_yet_is_checkpointed_as_such() {
    let processor = vec![Processor::new("steps", Events::changes("level"))];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    let mut restored = restore(&save(&live));

    // The type is still the first event's to give, before and after.
    for each in [&mut live, &mut restored] {
        each.on_point("level", TimePointEvent::new(2, 1.5_f64))
            .expect("the first event types the input");
        let mismatch = each.on_point("level", TimePointEvent::new(3, 4_i64));
        assert!(matches!(
            mismatch,
            Err(ProcessError::ValueTypeMismatch { .. })
        ));
    }
    assert_eq!(
        live.advance("level", 5).expect("known"),
        restored.advance("level", 5).expect("known")
    );

    // A typed input comes back typed.
    let typed = restore(&save(&restored));
    assert_eq!(typed.retained(), restored.retained());
}

#[test]
fn an_input_of_another_value_type_cannot_be_checkpointed() {
    let processor = vec![Processor::new("steps", Events::changes("level"))];
    let mut live = LiveProcessor::new(&processor, 0).expect("valid");
    live.on_point("level", TimePointEvent::new(2, 7_u8))
        .expect("any value type types an input");

    let error = serde_json::to_string(&live).expect_err("u8 is outside the closed set");
    let message = error.to_string();
    assert!(
        message.contains("level") && message.contains("u8"),
        "{message}"
    );
}

/// A processor of every kind that keeps state of its own.
fn entities() -> Vec<Processor> {
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    let present = Formula::between(direction("in"), direction("out"));
    let by = Group::new("gate", "agent");
    vec![
        Processor::new("presence", Recipe::per_entity(by.clone(), present.clone())),
        Processor::new("crowd", Recipe::count_entities(by, present)),
    ]
}

fn crossing(agent: i64, direction: &str) -> Value {
    Value::record([
        ("agent", Value::from(agent)),
        ("direction", Value::from(direction)),
    ])
}

fn entity_checkpoint() -> Json {
    let mut live = LiveProcessor::new(&entities(), 0).expect("valid");
    for (at, agent, way) in [(1, 7, "in"), (2, 9, "in"), (5, 7, "out")] {
        live.on_point("gate", TimePointEvent::new(at, crossing(agent, way)))
            .expect("on time");
    }
    live.advance("gate", 6).expect("known");
    serde_json::from_str(&save(&live)).expect("json")
}

#[test]
fn a_checkpoint_of_entities_keeps_each_of_them() {
    let mut live = LiveProcessor::new(&entities(), 0).expect("valid");
    live.on_point("gate", TimePointEvent::new(1, crossing(7, "in")))
        .expect("on time");
    live.advance("gate", 2).expect("known");
    let mut restored = restore(&save(&live));

    // An entity made after the restore is the one it would have been before.
    for each in [&mut live, &mut restored] {
        each.on_point("gate", TimePointEvent::new(3, crossing(9, "in")))
            .expect("on time");
    }
    assert_eq!(
        live.advance("gate", 8).expect("known"),
        restored.advance("gate", 8).expect("known")
    );
    assert_eq!(live.finish(10), restored.finish(10));
}

#[test]
fn a_checkpoint_of_another_format_version_is_refused() {
    let mut document = entity_checkpoint();
    assert_eq!(document["format"], json!(1));
    document["format"] = json!(2);
    assert!(refusal(&document).contains("format version 2"));
}

#[test]
fn a_cursor_past_its_frontier_is_refused() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.advance("count", 10).expect("known");
    let mut document: Json = serde_json::from_str(&save(&live)).expect("json");

    document["outputs"][0]["through"] = json!(50);
    assert!(
        refusal(&document).contains("past its frontier"),
        "{document}"
    );

    // Trusted from past the cursor is as wrong.
    let mut document: Json = serde_json::from_str(&save(&live)).expect("json");
    document["outputs"][0]["trusted"] = json!(11);
    assert!(refusal(&document).contains("trusted from"));
}

fn empty_checkpoint() -> Json {
    serde_json::from_str(&save(&LiveProcessor::new(&busy(), 0).expect("valid"))).expect("json")
}

#[test]
fn inputs_that_contradict_the_processor_are_refused() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.on_point("count", TimePointEvent::new(7, 9_i64))
        .expect("on time");
    let saved: Json = serde_json::from_str(&save(&live)).expect("json");

    let mut missing = saved.clone();
    missing["inputs"]["members"] = json!([]);
    assert!(refusal(&missing).contains("`count` is missing"));

    let mut stranger = saved.clone();
    stranger["inputs"]["members"][0]["key"] = json!("elsewhere");
    assert!(refusal(&stranger).contains("`count` is missing"));

    // With no event in it, an input of another type is a well-formed member.
    let mut mistyped: Json =
        serde_json::from_str(&save(&LiveProcessor::new(&busy(), 0).expect("valid"))).expect("json");
    mistyped["inputs"]["members"][0]["sequence"]["type"] = json!("text");
    assert!(refusal(&mistyped).contains("`count` holds"));

    let mut batch = empty_checkpoint();
    batch["inputs"]["members"][0]
        .as_object_mut()
        .expect("a member")
        .remove("frontier");
    assert!(refusal(&batch).contains("not live"));
}

#[test]
fn entities_and_their_views_must_agree() {
    let saved = entity_checkpoint();

    let mut repeated = saved.clone();
    let seen = repeated["groups"][0]["entities"][0].clone();
    repeated["groups"][0]["entities"]
        .as_array_mut()
        .expect("a list")
        .push(seen);
    assert!(refusal(&repeated).contains("twice"));

    let mut nameless = saved.clone();
    nameless["groups"][0]["entities"][0]["entity"] = json!(null);
    assert!(refusal(&nameless).contains("not a bool, a number or text"));

    // Swapping the entities of the views leaves records with the wrong owner.
    let mut swapped = saved.clone();
    let entities = swapped["groups"][0]["entities"]
        .as_array_mut()
        .expect("list");
    entities.swap(0, 1);
    assert!(refusal(&swapped).contains("another entity"));

    let mut early = saved.clone();
    early["groups"][0]["entities"][0]["first"] = json!(4);
    assert!(refusal(&early).contains("before its entity was first seen"));

    let mut elsewhere = saved.clone();
    elsewhere["groups"][0]["key"] = json!("elsewhere");
    let message = refusal(&elsewhere);
    assert!(message.contains("where `crowd` belongs"), "{message}");

    let mut counted = saved;
    counted["counts"] = json!([{"key": "presence", "points": []}]);
    assert!(refusal(&counted).contains("not a count of entities"));
}

#[test]
fn a_finished_checkpoint_with_open_cursors_is_refused() {
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    live.finish(10).expect("finishing");
    let mut document: Json = serde_json::from_str(&save(&live)).expect("json");
    document["outputs"][0]["through"] = json!(5);
    assert!(refusal(&document).contains("not final in a finished processor"));
}

#[test]
fn the_state_a_checkpoint_stores_is_the_retained_window() {
    // A long stream with spans that close checkpoints to a small document.
    let mut live = LiveProcessor::new(&busy(), 0).expect("valid");
    for tick in 0..2000 {
        live.on_point("count", TimePointEvent::new(tick, tick % 10))
            .expect("on time");
        live.advance("count", tick + 1).expect("known");
    }
    assert!(live.retained() < 10);
    assert!(save(&live).len() < 2000, "{} bytes", save(&live).len());
}

#[test]
fn a_processor_with_declared_inputs_checkpoints_and_keeps_its_declarations() {
    let readings = Schema::record([("count", Schema::Integer)]);
    let processor = vec![Processor::new(
        "crowded",
        Formula::while_matching(Pattern::field(
            "room",
            "count",
            Condition::Greater(Value::from(5_i64)),
        )),
    )
    .with_input("room", Input::Records(readings))
    // Declared but read by no formula: it is an input all the same.
    .with_input("spare", Input::Points(Kind::Float))];
    let record = |count: i64| Value::record([("count", Value::from(count))]);

    let mut uninterrupted = LiveProcessor::new(&processor, 0).expect("valid");
    let mut resumed = LiveProcessor::new(&processor, 0).expect("valid");
    for live in [&mut uninterrupted, &mut resumed] {
        live.on_point("room", TimePointEvent::new(2, record(7)))
            .unwrap();
        live.on_point("spare", TimePointEvent::new(3, 1.5_f64))
            .unwrap();
        live.advance("room", 5).unwrap();
    }

    let mut resumed = restore(&save(&resumed));
    assert_eq!(resumed.processors(), &processor);
    assert!(resumed.processors()[0].declared("spare").is_some());

    // The declaration still holds after the restore: a record off the schema
    // is refused, and the processor goes on as one that never stopped.
    let off_schema = Value::record([("count", Value::from("many"))]);
    assert!(resumed
        .on_point("room", TimePointEvent::new(6, off_schema))
        .is_err());
    for live in [&mut uninterrupted, &mut resumed] {
        live.on_point("room", TimePointEvent::new(8, record(1)))
            .unwrap();
    }
    assert_eq!(
        uninterrupted.advance("room", 12).unwrap(),
        resumed.advance("room", 12).unwrap()
    );
    assert_eq!(
        uninterrupted.finish(20).unwrap(),
        resumed.finish(20).unwrap()
    );
}

#[test]
fn a_checkpoint_whose_input_lost_its_declared_schema_is_refused() {
    let processor = vec![Processor::new(
        "crowded",
        Formula::while_matching(Pattern::field(
            "room",
            "count",
            Condition::Greater(Value::from(5_i64)),
        )),
    )
    .with_input(
        "room",
        Input::Records(Schema::record([("count", Schema::Integer)])),
    )];
    let live = LiveProcessor::new(&processor, 0).expect("valid");
    let mut document: Json = serde_json::from_str(&save(&live)).expect("json");

    let members = document["inputs"]["members"]
        .as_array_mut()
        .expect("the inputs are a system document");
    let room = members
        .iter_mut()
        .find(|member| member["key"] == "room")
        .expect("the declared input is stored");
    assert!(room.get("schema").is_some(), "{room}");
    room.as_object_mut()
        .expect("a member is an object")
        .remove("schema");

    let message = refusal(&document);
    assert!(
        message.contains("`room`") && message.contains("schema"),
        "{message}"
    );
}
