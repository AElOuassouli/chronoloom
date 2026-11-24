//! Checkpoints of a live stream system, behind the `serde` feature: a system
//! saved at any point and restored goes on exactly as one that never stopped,
//! and a document that contradicts itself is refused.
#![cfg(feature = "serde")]
#![allow(dead_code, reason = "each test file uses some of the shared helpers")]

#[path = "common/random.rs"]
mod common;
#[path = "common/streams.rs"]
mod streams;

use serde_json::{json, Value as Json};

use chronoloom::stream_system::{Live, LiveReport, StreamSystem, Update};
use chronoloom::{
    Condition, Events, Formula, Group, Kind, Pattern, Processor, Recipe, TimeIntervalSequence,
    TimePointEvent, TimePointSequence, Timestamp, Value,
};

use common::{for_each_seed, property_cases, span, Random};
use streams::{processors, stream, Op, Stream, POINT_INPUTS, RECORD_INPUTS, STATE_INPUTS};

/// The checkpoint of a live system, as text.
fn save(live: &StreamSystem<Live>) -> String {
    serde_json::to_string(live).expect("a checkpoint is written")
}

/// Restore a checkpoint, and check that saving the result gives the same text.
fn restore(text: &str) -> StreamSystem<Live> {
    let live: StreamSystem<Live> = serde_json::from_str(text).expect("a checkpoint loads");
    assert_eq!(save(&live), text, "a restored system saves what it was");
    live
}

fn refusal(document: &Json) -> String {
    match serde_json::from_value::<StreamSystem<Live>>(document.clone()) {
        Ok(_) => panic!("the document should be refused: {document}"),
        Err(error) => error.to_string(),
    }
}

/// A live system holding the streams the random processors read, empty, from
/// `from`, with the processors added.
fn system(from: Timestamp, processors: &[Processor]) -> StreamSystem<Live> {
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
    for processor in processors {
        system.add_processor(processor.clone()).expect("added");
    }
    system
}

fn apply(live: &mut StreamSystem<Live>, op: &Op) -> LiveReport {
    let mut update = Update::new();
    match op {
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
    live.update(update).expect("an update that is on time")
}

/// What a host sees of the run: every report, the parts drained on the way,
/// then the system as it ends.
type Seen = (Vec<LiveReport>, Vec<StreamSystem>, StreamSystem<Live>);

/// Run the stream, draining before the operations at `drains` and saving and
/// restoring before those at `stops`, then finish; every report is observed.
fn run(processors: &[Processor], stream: &Stream, drains: &[usize], stops: &[usize]) -> Seen {
    let mut live = system(stream.from, processors);
    let mut reports = Vec::new();
    let mut parts = Vec::new();

    let end = stream.ops.len();
    for index in 0..=end {
        for _ in drains.iter().filter(|drain| **drain == index) {
            parts.push(live.drain());
        }
        for _ in stops.iter().filter(|stop| **stop == index) {
            live = restore(&save(&live));
        }
        if let Some(op) = stream.ops.get(index) {
            reports.push(apply(&mut live, op));
        }
    }
    reports.push(live.finish(stream.horizon).expect("finishing cannot fail"));

    (reports, parts, live)
}

fn stop(random: &mut Random, stream: &Stream) -> usize {
    random.below(stream.ops.len() as u64 + 1) as usize
}

#[test]
fn a_restored_system_goes_on_as_one_that_never_stopped() {
    for_each_seed(property_cases(), |random, _| {
        let processors = processors(random);
        let stream = stream(random);

        // Some runs drain in the middle: a checkpoint then holds states whose
        // data is gone.
        let drains: Vec<usize> = if random.chance(1, 2) {
            vec![stop(random, &stream), stop(random, &stream)]
        } else {
            Vec::new()
        };
        let uninterrupted = run(&processors, &stream, &drains, &[]);

        // Once, at a random point; right at the start; and twice in one run,
        // the second time possibly right after the first.
        let first = stop(random, &stream);
        let second = stop(random, &stream);
        for stops in [vec![first], vec![0], vec![first, second]] {
            assert_eq!(
                run(&processors, &stream, &drains, &stops),
                uninterrupted,
                "drains before {drains:?}, checkpoints before the operations {stops:?} of {:?}",
                stream.ops
            );
        }
    });
}

#[test]
fn a_checkpoint_after_finish_stays_finished() {
    for_each_seed(property_cases(), |random, _| {
        let processors = processors(random);
        let stream = stream(random);
        let mut live = system(stream.from, &processors);
        for op in &stream.ops {
            apply(&mut live, op);
        }
        live.finish(stream.horizon).expect("finishing");

        let mut restored = restore(&save(&live));
        assert_eq!(restored, live);
        assert!(restored
            .finish(stream.horizon)
            .expect("finished")
            .is_empty());
        for key in live.derived() {
            assert_eq!(restored.frontier(key.as_str()), Some(Timestamp::MAX));
        }
        let mut late = Update::new();
        late.points("p", [TimePointEvent::new(stream.horizon, 1_i64)]);
        assert!(restored.update(late).is_err());
    });
}

fn busy() -> Processor {
    Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64))),
    )
}

fn counting(processor: Processor) -> StreamSystem<Live> {
    let mut live = StreamSystem::live(0);
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .expect("new");
    live.add_processor(processor).expect("added");
    live
}

#[test]
fn the_checkpoint_holds_its_processors_metadata_and_frontiers() {
    let mut live = counting(busy());
    live.metadata_mut()
        .insert("unit".to_owned(), "seconds".to_owned());
    let mut update = Update::new();
    update.points("count", [TimePointEvent::new(7, 9_i64)]);
    update.advance("count", 10);
    live.update(update).expect("on time");

    let restored = restore(&save(&live));
    assert_eq!(restored.processors(), live.processors());
    assert_eq!(restored.metadata(), live.metadata());
    assert_eq!(restored.frontier("count"), Some(10));
    assert_eq!(restored.frontier("busy"), live.frontier("busy"));
    assert_eq!(restored, live);
}

#[test]
fn a_stream_added_late_and_held_data_come_back() {
    let mut live = counting(busy());
    live.advance("count", 20).expect("known");
    live.insert_stream("later", TimePointSequence::<i64>::new("later"))
        .expect("new");

    // Data past the frontier is held, not final, and the checkpoint keeps it.
    let mut update = Update::new();
    update.points("count", [TimePointEvent::new(30, 8_i64)]);
    live.update(update).expect("on time");

    let mut restored = restore(&save(&live));
    assert_eq!(restored.frontier("later"), Some(0));
    assert_eq!(restored.get("count"), live.get("count"));
    assert_eq!(restored.finish(40), live.finish(40));
}

fn entities() -> StreamSystem<Live> {
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    let present = Formula::between(direction("in"), direction("out"));
    let by = Group::new("gate", "agent");
    let mut live = StreamSystem::live(0);
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .expect("new");
    live.add_processor(Processor::new(
        "presence",
        Recipe::per_entity(by.clone(), present.clone()),
    ))
    .expect("added");
    live.add_processor(Processor::new("crowd", Recipe::count_entities(by, present)))
        .expect("added");
    live
}

fn crossing(agent: i64, direction: &str) -> Value {
    Value::record([
        ("agent", Value::from(agent)),
        ("direction", Value::from(direction)),
    ])
}

#[test]
fn a_checkpoint_of_entities_keeps_each_of_them() {
    let mut live = entities();
    let mut update = Update::new();
    update.points("gate", [TimePointEvent::new(1, crossing(7, "in"))]);
    update.advance("gate", 2);
    live.update(update).expect("on time");
    let mut restored = restore(&save(&live));

    // An entity made after the restore is the one it would have been before.
    for each in [&mut live, &mut restored] {
        let mut update = Update::new();
        update.points("gate", [TimePointEvent::new(3, crossing(9, "in"))]);
        update.advance("gate", 8);
        each.update(update).expect("on time");
    }
    assert_eq!(live, restored);
    assert_eq!(live.derived().count(), restored.derived().count());
    assert_eq!(live.finish(10), restored.finish(10));
}

#[test]
fn a_transform_is_not_run_by_a_live_system_so_there_is_none_to_save() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("level", TimePointSequence::<f64>::new("level"))
        .expect("new");
    assert!(live
        .add_processor(Processor::new("level", Events::changes("level")))
        .is_err());
}

fn checkpoint() -> Json {
    let mut live = entities();
    let mut update = Update::new();
    update.points(
        "gate",
        [
            TimePointEvent::new(1, crossing(7, "in")),
            TimePointEvent::new(2, crossing(9, "in")),
            TimePointEvent::new(5, crossing(7, "out")),
        ],
    );
    update.advance("gate", 6);
    live.update(update).expect("on time");
    serde_json::from_str(&save(&live)).expect("json")
}

#[test]
fn the_document_is_tagged_live_and_refused_as_batch_and_the_reverse() {
    let document = checkpoint();
    assert_eq!(document["format"], json!(1));
    assert_eq!(document["mode"], json!("live"));
    assert_eq!(document["from"], json!(0));

    let batch = serde_json::from_value::<StreamSystem>(document.clone()).expect_err("refused");
    assert!(batch.to_string().contains("`live` mode"), "{batch}");

    let batch_document = serde_json::to_value(StreamSystem::new()).expect("a batch document");
    let message = refusal(&batch_document);
    assert!(
        message.contains("`batch` mode") && message.contains("into_live"),
        "{message}"
    );

    let mut future = document;
    future["format"] = json!(2);
    assert!(refusal(&future).contains("format version 2"));
}

#[test]
fn a_document_that_contradicts_itself_is_refused() {
    let saved = checkpoint();

    let mut without = saved.clone();
    without["streams"][0]
        .as_object_mut()
        .expect("a stream")
        .remove("frontier");
    assert!(refusal(&without).contains("has no frontier"));

    let mut origin = saved.clone();
    origin.as_object_mut().expect("a document").remove("from");
    assert!(refusal(&origin).contains("from"));

    // A frontier behind where the system started.
    let mut behind = saved.clone();
    behind["from"] = json!(3);
    behind["streams"][0]["frontier"] = json!(1);
    assert!(refusal(&behind).contains("before where the system started"));

    let mut twice = saved.clone();
    let repeated = twice["streams"][0].clone();
    twice["streams"]
        .as_array_mut()
        .expect("streams")
        .push(repeated);
    assert!(refusal(&twice).contains("twice"));

    // A processor that reads a stream the table does not hold.
    let mut orphan = saved.clone();
    orphan["streams"] = json!([]);
    assert!(refusal(&orphan).contains("processor `presence`"));

    let mut missing = saved.clone();
    missing["nodes"].as_array_mut().expect("nodes").pop();
    assert!(refusal(&missing).contains("running state"));

    let mut nodeless = saved.clone();
    nodeless
        .as_object_mut()
        .expect("a document")
        .remove("nodes");
    assert!(refusal(&nodeless).contains("nodes"));

    // The running state of one processor under another.
    let mut swapped = saved.clone();
    swapped["nodes"].as_array_mut().expect("nodes").swap(0, 1);
    assert!(refusal(&swapped).contains("another kind"));

    let mut lost = saved.clone();
    lost["derived"].as_array_mut().expect("derived").pop();
    assert!(refusal(&lost).contains("disagree"));

    let mut stranger = saved.clone();
    stranger["derived"][0]["key"] = json!("elsewhere");
    assert!(refusal(&stranger).contains("disagree") || refusal(&stranger).contains("no processor"));

    let mut ahead = saved.clone();
    ahead["nodes"][0]["read"] = json!({"gate": 99});
    assert!(refusal(&ahead).contains("past its frontier"));

    let mut schema = saved;
    schema["streams"][0]["schema"] = json!("integer");
    assert!(refusal(&schema).contains("schema"));
}

#[test]
fn an_input_of_another_value_type_cannot_be_checkpointed() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("level", TimePointSequence::<u8>::new("level"))
        .expect("new");

    let message = serde_json::to_string(&live)
        .expect_err("u8 is outside the closed set")
        .to_string();
    assert!(
        message.contains("level") && message.contains("u8"),
        "{message}"
    );
}

#[test]
fn a_drained_system_is_checkpointed_with_its_running_state() {
    let mut live = counting(busy());
    let mut update = Update::new();
    update.points(
        "count",
        [
            TimePointEvent::new(3, 9_i64),
            TimePointEvent::new(14, 1_i64),
        ],
    );
    update.advance("count", 10);
    live.update(update).expect("on time");
    let _final = live.drain();

    // The span that began at 3 is still open: only the state knows it.
    let mut restored = restore(&save(&live));
    assert_eq!(restored, live);
    assert_eq!(restored.finish(30), live.finish(30));
    assert_eq!(restored, live);
}

#[test]
fn a_batch_system_goes_live_through_into_live_and_checkpoints() {
    let mut batch = StreamSystem::new();
    batch
        .insert_stream(
            "count",
            TimePointSequence::from_events(
                "count",
                vec![
                    TimePointEvent::new(3, 9_i64),
                    TimePointEvent::new(12, 1_i64),
                ],
            ),
        )
        .expect("new");
    batch.add_processor(busy()).expect("added");

    let live = batch.into_live(5).expect("live");
    let restored = restore(&save(&live));
    assert_eq!(restored, live);
    assert_eq!(restored.get("busy"), live.get("busy"));
}

#[test]
fn a_document_whose_derived_streams_or_state_misbehave_is_refused() {
    let saved = checkpoint();
    let position = |key: &str| {
        saved["derived"]
            .as_array()
            .expect("derived")
            .iter()
            .position(|member| member["key"] == json!(key))
            .expect("a derived stream")
    };

    // A derived stream observed further than its processor has read.
    let mut past = saved.clone();
    past["derived"][position("presence/7")]["sequence"]["observed"] = json!([[1, 99]]);
    assert!(refusal(&past).contains("past its frontier"));

    // An entity named on a stream that has none, and the reverse.
    let mut named = saved.clone();
    named["derived"][position("crowd")]["entity"] = json!(7);
    assert!(refusal(&named).contains("disagree"));
    let mut unnamed = saved.clone();
    unnamed["derived"][position("presence/7")]
        .as_object_mut()
        .expect("a member")
        .remove("entity");
    assert!(refusal(&unnamed).contains("disagree"));

    // An entity named on the stream of another.
    let mut swapped = saved.clone();
    swapped["derived"][position("presence/7")]["entity"] = json!(9);
    assert!(refusal(&swapped).contains("disagree"));

    // A derived stream no processor writes.
    let mut stray = saved.clone();
    let extra = stray["derived"][position("crowd")].clone();
    let mut extra = extra;
    extra["key"] = json!("nobody");
    stray["derived"]
        .as_array_mut()
        .expect("derived")
        .push(extra);
    assert!(refusal(&stray).contains("nobody"));

    // A state that has read a stream its processor does not read.
    let mut reads = saved.clone();
    reads["nodes"][0]["read"] = json!({"elsewhere": 1});
    assert!(refusal(&reads).contains("which it does not"));

    // A state that holds a formula other than its processor's.
    let mut formula = saved.clone();
    let text = formula["nodes"].to_string();
    assert!(text.contains("\"equals\""), "{text}");
    formula["nodes"] = serde_json::from_str(&text.replace("\"in\"", "\"out\"")).expect("json");
    assert!(refusal(&formula).contains("other than the processor's"));

    // A processor missing from the document, its state left behind.
    let mut fewer = saved;
    fewer["processors"]
        .as_array_mut()
        .expect("processors")
        .pop();
    assert!(refusal(&fewer).contains("running state"));
}

#[test]
fn an_end_edge_may_lie_outside_what_was_observed_and_nothing_else_may() {
    // The edges of a state: a span that stops may leave its `end` edge where
    // nothing was observed. The checkpoint must carry it through.
    let mut live = StreamSystem::live(0);
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .expect("new");
    live.add_processor(Processor::new(
        "edges",
        Events::Edges(Formula::while_matching(Pattern::new(
            "count",
            Condition::Greater(5_i64),
        ))),
    ))
    .expect("added");
    let mut update = Update::new();
    update.points(
        "count",
        [TimePointEvent::new(3, 9_i64), TimePointEvent::new(8, 1_i64)],
    );
    update.advance("count", 10);
    live.update(update).expect("on time");
    let saved: Json = serde_json::from_str(&save(&live)).expect("json");
    let edges = &saved["derived"][0]["sequence"]["events"];
    assert!(edges.as_array().expect("events").len() >= 2, "{saved}");
    let end = edges
        .as_array()
        .expect("events")
        .iter()
        .position(|event| event["value"] == json!("end"))
        .expect("an end edge");

    let mut moved = saved.clone();
    moved["derived"][0]["sequence"]["events"][end]["timestamp"] = json!(500);
    let loaded: StreamSystem<Live> =
        serde_json::from_value(moved).expect("an end edge outside the extent loads");
    assert_eq!(loaded.frontier("edges"), live.frontier("edges"));

    let start = 1 - end.min(1);
    let mut stray = saved;
    stray["derived"][0]["sequence"]["events"][start]["timestamp"] = json!(500);
    assert!(refusal(&stray).contains("outside the extent"));
}

#[test]
fn a_timeline_or_chunk_outside_its_range_is_refused() {
    let mut live = counting(busy());
    let mut update = Update::new();
    update.points("count", [TimePointEvent::new(3, 9_i64)]);
    update.advance("count", 10);
    live.update(update).expect("on time");
    let saved: Json = serde_json::from_str(&save(&live)).expect("json");
    assert_eq!(restore(&save(&live)), live);

    // A span past what the timeline observed.
    let mut extended = saved.clone();
    extended["derived"][0]["sequence"]["spans"] = json!([[3, 50]]);
    assert!(refusal(&extended).contains("outside the extent"));

    // A chunk, which is what the report of an update gives, and a state holds.
    let mut update = Update::new();
    update.points("count", [TimePointEvent::new(12, 1_i64)]);
    update.advance("count", 20);
    let report = live.update(update).expect("on time");
    let chunk = report.chunks().values().next().expect("progress");
    let text = serde_json::to_string(chunk).expect("a chunk is written");
    assert_eq!(
        &serde_json::from_str::<chronoloom::stream_system::Chunk>(&text).expect("loads"),
        chunk
    );
    let mut document: Json = serde_json::from_str(&text).expect("json");

    let range = chunk.range();
    document["to"] = json!(range.start - 1);
    let error = serde_json::from_value::<chronoloom::stream_system::Chunk>(document.clone());
    assert!(error
        .expect_err("backwards")
        .to_string()
        .contains("before it starts"));

    document["to"] = json!(range.end);
    document["from"] = json!(range.start + 1);
    let error = serde_json::from_value::<chronoloom::stream_system::Chunk>(document);
    assert!(error.expect_err("outside").to_string().contains("outside"));
}

/// The processors that cover every kind of running state.
fn every_state() -> Vec<Processor> {
    let door = || Pattern::new("count", Condition::Greater(5_i64));
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    let present = || Formula::between(direction("in"), direction("out"));
    let by = || Group::new("gate", "agent");
    vec![
        Processor::new("busy", Formula::while_matching(door())),
        Processor::new("held", Formula::while_within(door(), 4)),
        Processor::new("seen", Formula::occurs(door()).shift(2, 3)),
        Processor::new(
            "mixed",
            Formula::state("busy")
                .and(!Formula::state("held"))
                .or(Formula::state("seen")),
        ),
        Processor::new("edges", Events::Edges(Formula::state("mixed"))),
        Processor::new("steps", Events::changes("count")),
        Processor::new("agents", Events::project("gate", "agent", Kind::Integer)),
        Processor::new("presence", Recipe::per_entity(by(), present())),
        Processor::new("anyone", Recipe::any_entity(by(), present())),
        Processor::new("crowd", Recipe::count_entities(by(), present())),
    ]
}

#[test]
fn the_document_of_a_live_system_is_the_one_stored() {
    let mut live = StreamSystem::live(0);
    live.insert_stream("count", TimePointSequence::<i64>::new("count"))
        .expect("new");
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .expect("new");
    for processor in every_state() {
        live.add_processor(processor).expect("added");
    }
    let mut update = Update::new();
    update.points(
        "count",
        [
            TimePointEvent::new(2, 9_i64),
            TimePointEvent::new(6, 1_i64),
            TimePointEvent::new(9, 7_i64),
        ],
    );
    update.points(
        "gate",
        [
            TimePointEvent::new(1, crossing(7, "in")),
            TimePointEvent::new(4, crossing(9, "in")),
            TimePointEvent::new(8, crossing(7, "out")),
        ],
    );
    update.advance("count", 8);
    update.advance("gate", 5);
    live.update(update).expect("on time");

    let document: Json = serde_json::from_str(&save(&live)).expect("json");
    assert_eq!(restore(&save(&live)), live);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/live_checkpoint.json");
    if std::env::var_os("CHRONOLOOM_UPDATE_SCHEMA").is_some() {
        let text = serde_json::to_string_pretty(&document).expect("text") + "\n";
        std::fs::write(&path, text).expect("the fixture is written");
        return;
    }
    let stored = std::fs::read_to_string(&path)
        .expect("tests/fixtures/live_checkpoint.json is missing; see the serialisation module");
    let stored: Json = serde_json::from_str(&stored).expect("json");
    assert_eq!(
        document, stored,
        "the document of a live system changed. The running state is part of format 1: decide \
         whether the format number must change, then run the tests again with \
         CHRONOLOOM_UPDATE_SCHEMA=1 to rewrite the fixture"
    );

    // And the stored document still loads and goes on.
    let mut loaded: StreamSystem<Live> = serde_json::from_value(stored).expect("it loads");
    assert_eq!(loaded, live);
    assert_eq!(loaded.finish(20), live.finish(20));
}

/// The processors of a plan, and of the other one.
type Plans<'a> = Box<dyn Fn(bool) -> Vec<Processor> + 'a>;

/// A live system over `count`, `other` and `gate`, with the processors, and
/// some data read.
fn plan_system(processors: &[Processor]) -> StreamSystem<Live> {
    let mut live = StreamSystem::live(0);
    for key in ["count", "other"] {
        live.insert_stream(key, TimePointSequence::<i64>::new(key))
            .expect("new");
    }
    live.insert_stream("gate", TimePointSequence::<Value>::new("gate"))
        .expect("new");
    for processor in processors {
        live.add_processor(processor.clone()).expect("added");
    }
    let mut update = Update::new();
    update.points(
        "count",
        [TimePointEvent::new(2, 9_i64), TimePointEvent::new(6, 1_i64)],
    );
    update.points("other", [TimePointEvent::new(3, 4_i64)]);
    update.points(
        "gate",
        [
            TimePointEvent::new(1, crossing(7, "in")),
            TimePointEvent::new(4, crossing(9, "in")),
        ],
    );
    update.advance("count", 8);
    update.advance("other", 8);
    update.advance("gate", 8);
    live.update(update).expect("on time");
    live
}

/// Load the state saved under one plan beside the processors of another.
fn under_other_plan(make: impl Fn(bool) -> Vec<Processor>) -> String {
    let mut document: Json = serde_json::from_str(&save(&plan_system(&make(false)))).expect("json");
    let other: Json = serde_json::from_str(&save(&plan_system(&make(true)))).expect("json");
    // The same processors are accepted.
    let same: Json = serde_json::from_str(&save(&plan_system(&make(false)))).expect("json");
    serde_json::from_value::<StreamSystem<Live>>(same).expect("the same plan loads");
    document["processors"] = other["processors"].clone();
    refusal(&document)
}

#[test]
fn a_state_under_a_processor_of_another_plan_is_refused_for_every_kind() {
    let door = |threshold: i64| Pattern::new("count", Condition::Greater(threshold));
    let direction = |text: &'static str| {
        Pattern::field("gate", "direction", Condition::Equals(Value::from(text)))
    };
    let base = || {
        vec![
            Processor::new("busy", Formula::while_matching(door(5))),
            Processor::new("held", Formula::while_within(door(5), 4)),
        ]
    };
    let with = |last: Processor| {
        let mut processors = base();
        processors.push(last);
        processors
    };
    let pick = |other: bool, first: Formula, second: Formula| {
        if other {
            second
        } else {
            first
        }
    };
    let group = |other: bool| Group::new("gate", if other { "unit" } else { "agent" });

    let cases: Vec<(&str, Plans<'_>)> = vec![
        (
            "while",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Formula::while_matching(door(if other { 6 } else { 5 })),
                ))
            }),
        ),
        (
            "within",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Formula::while_within(door(5), if other { 5 } else { 4 }),
                ))
            }),
        ),
        (
            "shift",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Formula::occurs(door(5)).shift(2, if other { 4 } else { 3 }),
                ))
            }),
        ),
        (
            "occurs",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Formula::occurs(door(if other { 6 } else { 5 })),
                ))
            }),
        ),
        (
            "state",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Formula::state(if other { "held" } else { "busy" }),
                ))
            }),
        ),
        (
            "and",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    pick(
                        other,
                        Formula::state("busy").and(Formula::state("held")),
                        Formula::state("busy").and(!Formula::state("held")),
                    ),
                ))
            }),
        ),
        (
            "or",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    pick(
                        other,
                        Formula::state("busy").or(Formula::state("held")),
                        Formula::state("busy").or(!Formula::state("held")),
                    ),
                ))
            }),
        ),
        (
            "not",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    !Formula::state(if other { "held" } else { "busy" }),
                ))
            }),
        ),
        (
            "edges",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Events::Edges(Formula::state(if other { "held" } else { "busy" })),
                ))
            }),
        ),
        (
            "changes",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Events::changes(if other { "other" } else { "count" }),
                ))
            }),
        ),
        (
            "project",
            Box::new(|other| {
                with(Processor::new(
                    "x",
                    Events::project("gate", if other { "unit" } else { "agent" }, Kind::Integer),
                ))
            }),
        ),
        (
            "per entity",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    Recipe::per_entity(
                        group(other),
                        Formula::between(direction("in"), direction("out")),
                    ),
                ))
            }),
        ),
        (
            "per entity formula",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    Recipe::per_entity(
                        group(false),
                        Formula::between(
                            direction("in"),
                            direction(if other { "gone" } else { "out" }),
                        ),
                    ),
                ))
            }),
        ),
        (
            "any entity",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    Recipe::any_entity(
                        group(false),
                        Formula::between(
                            direction("in"),
                            direction(if other { "gone" } else { "out" }),
                        ),
                    ),
                ))
            }),
        ),
        (
            "count entities",
            Box::new(move |other| {
                with(Processor::new(
                    "x",
                    Recipe::count_entities(
                        group(other),
                        Formula::between(direction("in"), direction("out")),
                    ),
                ))
            }),
        ),
    ];
    for (name, make) in cases {
        let message =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| under_other_plan(&*make)));
        assert!(message.is_ok(), "{name}: the other plan is not refused");
    }
}

#[test]
fn an_entity_that_does_not_match_its_key_is_refused() {
    let saved = checkpoint();
    let index = saved["derived"]
        .as_array()
        .expect("derived")
        .iter()
        .position(|member| member["key"] == json!("presence/7"))
        .expect("a member");

    let mut other = saved.clone();
    other["derived"][index]["entity"] = json!(8);
    let message = refusal(&other);
    assert!(
        message.contains("entit") || message.contains("disagree"),
        "{message}"
    );

    let mut kind = saved;
    kind["derived"][index]["entity"] = json!("7");
    refusal(&kind);
}
