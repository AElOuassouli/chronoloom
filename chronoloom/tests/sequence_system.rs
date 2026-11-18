use std::ops::Bound;

use chronoloom::primitives::{IntervalError, TimeIntervalEvent, TimePointEvent};
use chronoloom::sequence_system::{
    AnyPointSequence, Batch, Key, RecordError, Sequence, SequenceSystem,
};
use chronoloom::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use chronoloom::{
    Condition, Edge, Events, Formula, Output, Pattern, ProcessError, Processor, Schema,
    SchemaError, Value,
};

/// A span that is known to be valid.
fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).expect("test spans are valid")
}

/// A point sequence for `attribute` holding `(timestamp, value)` pairs.
fn points<T>(attribute: &str, events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
    TimePointSequence::from_events(
        attribute,
        events
            .into_iter()
            .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
            .collect(),
    )
}

/// A system with an `f64` temperature, a `String` log, and an `up` timeline,
/// all empty.
fn machine() -> SequenceSystem {
    let mut machine = SequenceSystem::new();
    machine.insert("temperature", TimePointSequence::<f64>::new("temperature"));
    machine.insert("log", TimePointSequence::<String>::new("log"));
    machine.insert("up", TimeIntervalSequence::new("up"));

    machine
}

/// The timestamps the point sequence for `key` holds, in order.
fn timestamps<T: 'static>(system: &SequenceSystem, key: &str) -> Vec<i64> {
    system
        .points::<T>(key)
        .expect("the sequence exists with this value type")
        .iter()
        .map(TimePointEvent::timestamp)
        .collect()
}

/// The bounds the timeline for `key` holds, in order.
fn bounds(system: &SequenceSystem, key: &str) -> Vec<(i64, i64)> {
    system
        .intervals(key)
        .expect("the timeline exists")
        .iter()
        .map(TimeIntervalEvent::bounds)
        .collect()
}

#[test]
fn a_new_system_holds_nothing() {
    let system = SequenceSystem::new();

    assert!(system.is_empty());
    assert_eq!(system.len(), 0);
    assert_eq!(system.iter().count(), 0);
    assert_eq!(system, SequenceSystem::default());
}

#[test]
fn sequences_of_both_shapes_and_several_value_types_sit_together() {
    let system = machine();

    assert_eq!(system.len(), 3);
    assert!(system.contains("temperature"));
    assert!(system.points::<f64>("temperature").is_some());
    assert!(system.points::<String>("log").is_some());
    assert!(system.intervals("up").is_some());
}

#[test]
fn iteration_is_in_key_order_however_the_system_was_built() {
    let system = machine();
    let keys: Vec<&str> = system.keys().map(Key::as_str).collect();

    assert_eq!(keys, ["log", "temperature", "up"]);
}

#[test]
fn typed_reads_refuse_the_wrong_type_shape_or_key() {
    let system = machine();

    assert!(system.points::<i64>("temperature").is_none());
    assert!(system.points::<f64>("up").is_none());
    assert!(system.intervals("temperature").is_none());
    assert!(system.points::<f64>("humidity").is_none());
    assert!(system.get("humidity").is_none());
}

#[test]
fn a_sequence_is_found_under_the_key_it_was_inserted_with() {
    let a = TimeIntervalSequence::from_spans("A", vec![span(0, 10)]);
    let b = TimeIntervalSequence::from_spans("B", vec![span(20, 30)]);

    let mut system = SequenceSystem::new();
    system.insert("union", a.union(&b));
    system.insert(
        String::from("shifted"),
        a.transform(1, 2).expect("no overflow"),
    );

    assert_eq!(
        system.intervals("union").map(TimeIntervalSequence::label),
        Some(String::from("A ∪ B"))
    );
    assert_eq!(
        system.intervals("shifted").map(TimeIntervalSequence::label),
        Some(String::from("A[1, 2]"))
    );
    assert!(system.intervals("A ∪ B").is_none());
}

#[test]
fn members_with_one_label_and_different_keys_coexist() {
    let mut system = SequenceSystem::new();
    system.insert("sensor-1", points("temperature", [(10, 1.0_f64)]));
    system.insert("sensor-2", points("temperature", [(20, 2.0_f64)]));

    assert_eq!(system.len(), 2);
    assert_eq!(timestamps::<f64>(&system, "sensor-1"), [10]);
    assert_eq!(timestamps::<f64>(&system, "sensor-2"), [20]);
    assert_eq!(
        system.get("sensor-1").map(Sequence::label),
        system.get("sensor-2").map(Sequence::label)
    );
}

#[test]
fn iteration_follows_the_keys_whatever_the_labels() {
    let mut system = SequenceSystem::new();
    system.insert("b", TimeIntervalSequence::new("a"));
    system.insert("a", TimeIntervalSequence::new("z"));

    let labels: Vec<(&str, String)> = system
        .iter()
        .map(|(key, sequence)| (key.as_str(), sequence.label()))
        .collect();
    assert_eq!(labels, [("a", String::from("z")), ("b", String::from("a"))]);
}

#[test]
fn relabel_changes_the_label_and_keeps_the_key_and_the_data() -> Result<(), RecordError> {
    let mut system = machine();
    system.record_point("temperature", TimePointEvent::new(10, 21.5_f64))?;
    system.record_span("up", span(0, 10))?;

    system.relabel("temperature", "air temperature")?;
    system.relabel("up", String::from("uptime"))?;

    assert_eq!(
        system.get("temperature").map(Sequence::label),
        Some(String::from("air temperature"))
    );
    assert_eq!(
        system.get("up").map(Sequence::label),
        Some(String::from("uptime"))
    );
    assert_eq!(timestamps::<f64>(&system, "temperature"), [10]);
    assert_eq!(bounds(&system, "up"), [(0, 10)]);
    assert!(system.get("air temperature").is_none());
    assert_eq!(system.len(), 3);
    Ok(())
}

#[test]
fn relabelling_an_unknown_key_changes_nothing() {
    let mut system = machine();
    let before = system.clone();

    assert_eq!(
        system.relabel("humidity", "wetness"),
        Err(RecordError::Unknown {
            key: String::from("humidity")
        })
    );
    assert_eq!(system, before);
}

#[test]
fn equality_compares_keys_and_labels() {
    let mut one = SequenceSystem::new();
    one.insert("x", points("a", [(10, 1)]));
    let mut other_key = SequenceSystem::new();
    other_key.insert("y", points("a", [(10, 1)]));
    let mut other_label = SequenceSystem::new();
    other_label.insert("x", points("b", [(10, 1)]));

    assert_ne!(one, other_key);
    assert_ne!(one, other_label);
    assert_eq!(one, one.clone());
}

#[test]
fn inserting_under_a_recorded_key_returns_what_it_displaced() {
    let mut system = SequenceSystem::new();

    assert!(system
        .insert("temperature", points("temperature", [(10, 1.0_f64)]))
        .is_none());
    let displaced = system.insert("temperature", points("temperature", [(20, 2.0_f64)]));

    let displaced = displaced.expect("the key was taken");
    assert_eq!(
        displaced.as_time_point::<f64>().map(|s| s[0].timestamp()),
        Some(10)
    );
    assert_eq!(system.len(), 1);
    assert_eq!(timestamps::<f64>(&system, "temperature"), [20]);
}

#[test]
fn remove_takes_the_sequence_out() {
    let mut system = machine();

    let removed = system.remove("up").expect("the timeline exists");

    assert!(removed.as_time_interval().is_some());
    assert!(!system.contains("up"));
    assert!(system.remove("up").is_none());
    assert_eq!(system.len(), 2);
}

#[test]
fn clear_empties_the_system() {
    let mut system = machine();
    system.clear();

    assert!(system.is_empty());
}

#[test]
fn recorded_points_stay_in_time_order() {
    let mut system = machine();

    for timestamp in [10, 30, 20] {
        system
            .record_point("temperature", TimePointEvent::new(timestamp, 0.0_f64))
            .expect("the sequence holds f64 points");
    }

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 20, 30]);
}

#[test]
fn recorded_spans_merge() -> Result<(), RecordError> {
    let mut system = machine();

    system.record_span("up", span(0, 5))?;
    system.record_span("up", span(5, 9))?;
    system.record_span("up", span(20, 30))?;

    assert_eq!(bounds(&system, "up"), [(0, 9), (20, 30)]);
    Ok(())
}

#[test]
fn recording_for_an_unknown_key_creates_nothing() {
    let mut system = machine();
    let before = system.clone();

    let refused = system.record_point("humidity", TimePointEvent::new(10, 0.4_f64));

    assert_eq!(
        refused,
        Err(RecordError::Unknown {
            key: String::from("humidity")
        })
    );
    assert_eq!(system, before);
}

#[test]
fn recording_the_wrong_shape_is_refused() {
    let mut system = machine();
    let before = system.clone();

    assert_eq!(
        system.record_point("up", TimePointEvent::new(10, true)),
        Err(RecordError::ShapeMismatch {
            key: String::from("up")
        })
    );
    assert_eq!(
        system.record_span("temperature", span(0, 10)),
        Err(RecordError::ShapeMismatch {
            key: String::from("temperature")
        })
    );
    assert_eq!(system, before);
}

#[test]
fn recording_the_wrong_value_type_is_refused() {
    let mut system = machine();
    let before = system.clone();

    let refused = system.record_point("temperature", TimePointEvent::new(10, 21_i64));

    assert_eq!(
        refused,
        Err(RecordError::ValueTypeMismatch {
            key: String::from("temperature"),
            expected: std::any::type_name::<f64>(),
            found: std::any::type_name::<i64>(),
        })
    );
    assert_eq!(system, before);
}

#[test]
fn record_errors_describe_themselves() {
    let unknown = RecordError::Unknown {
        key: String::from("humidity"),
    };

    assert_eq!(
        unknown.to_string(),
        "no sequence for `humidity` in the system"
    );
    assert_eq!(
        mismatch("sensor").to_string(),
        "a value does not have the schema of `sensor`, at `distance`: expected integer, found text"
    );
}

#[test]
fn a_batch_lands_in_the_sequence_for_each_attribute() -> Result<(), RecordError> {
    let mut system = machine();

    let mut batch = Batch::new();
    batch
        .points(
            "temperature",
            [
                TimePointEvent::new(11, 2.0_f64),
                TimePointEvent::new(10, 1.0),
            ],
        )
        .points("log", [TimePointEvent::new(10, String::from("boot"))])
        .spans("up", [span(0, 5), span(5, 10)]);
    assert_eq!(batch.len(), 5);

    system.record_batch(batch)?;

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 11]);
    assert_eq!(timestamps::<String>(&system, "log"), [10]);
    assert_eq!(bounds(&system, "up"), [(0, 10)]);
    Ok(())
}

#[test]
fn a_batch_may_push_to_one_attribute_twice() -> Result<(), RecordError> {
    let mut system = machine();

    let mut batch = Batch::new();
    batch
        .points("temperature", [TimePointEvent::new(20, 2.0_f64)])
        .points("temperature", [TimePointEvent::new(10, 1.0_f64)]);
    system.record_batch(batch)?;

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 20]);
    Ok(())
}

#[test]
fn an_empty_batch_records_nothing() -> Result<(), RecordError> {
    let mut system = machine();
    let before = system.clone();

    let batch = Batch::new();
    assert!(batch.is_empty());
    system.record_batch(batch)?;

    assert_eq!(system, before);
    Ok(())
}

#[test]
fn one_bad_push_rejects_the_whole_batch() {
    let bad_pushes: [fn(&mut Batch); 3] = [
        |batch| {
            batch.points("humidity", [TimePointEvent::new(10, 0.4_f64)]);
        },
        |batch| {
            batch.spans("temperature", [span(0, 10)]);
        },
        |batch| {
            batch.points("log", [TimePointEvent::new(10, 'x')]);
        },
    ];

    for bad_push in bad_pushes {
        let mut system = machine();
        let before = system.clone();

        let mut batch = Batch::new();
        batch
            .points("temperature", [TimePointEvent::new(10, 21.5_f64)])
            .spans("up", [span(0, 10)]);
        bad_push(&mut batch);

        assert!(system.record_batch(batch).is_err());
        assert_eq!(system, before);
    }
}

#[test]
fn a_batch_reports_the_first_bad_push() {
    let mut system = machine();

    let mut batch = Batch::new();
    batch
        .spans("temperature", [span(0, 10)])
        .points("humidity", [TimePointEvent::new(10, 0.4_f64)]);

    assert_eq!(
        system.record_batch(batch),
        Err(RecordError::ShapeMismatch {
            key: String::from("temperature")
        })
    );
}

#[test]
fn equality_ignores_insertion_order() {
    let forward: SequenceSystem = [
        ("a", Sequence::from(points("a", [(10, 1.0_f64)]))),
        (
            "b",
            Sequence::from(TimeIntervalSequence::from_spans("b", vec![span(0, 10)])),
        ),
    ]
    .into_iter()
    .collect();
    let backward: SequenceSystem = [
        (
            "b",
            Sequence::from(TimeIntervalSequence::from_spans("b", vec![span(0, 10)])),
        ),
        ("a", Sequence::from(points("a", [(10, 1.0_f64)]))),
    ]
    .into_iter()
    .collect();

    assert_eq!(forward, backward);
}

#[test]
fn same_key_with_another_value_type_is_not_equal() {
    let floats: SequenceSystem = [("reading", TimePointSequence::<f64>::new("reading"))]
        .into_iter()
        .collect();
    let integers: SequenceSystem = [("reading", TimePointSequence::<i64>::new("reading"))]
        .into_iter()
        .collect();

    assert_ne!(floats, integers);
}

#[test]
fn a_clone_is_equal_and_independent() -> Result<(), RecordError> {
    let original = machine();
    let mut copy = original.clone();
    assert_eq!(copy, original);

    copy.record_point("temperature", TimePointEvent::new(10, 1.0_f64))?;

    assert_ne!(copy, original);
    assert_eq!(
        timestamps::<f64>(&original, "temperature"),
        Vec::<i64>::new()
    );
    Ok(())
}

#[test]
fn collecting_and_extending_keep_the_later_of_two_sequences_for_one_key() {
    let mut system: SequenceSystem = [
        ("reading", points("reading", [(10, 1)])),
        ("reading", points("reading", [(20, 2)])),
    ]
    .into_iter()
    .collect();
    assert_eq!(timestamps::<i32>(&system, "reading"), [20]);

    system.extend([("reading", points("reading", [(30, 3)]))]);
    assert_eq!(timestamps::<i32>(&system, "reading"), [30]);
}

#[test]
fn consuming_iteration_yields_owned_sequences_in_key_order() {
    let owned: Vec<(Key, Sequence)> = machine().into_iter().collect();

    let keys: Vec<&str> = owned.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["log", "temperature", "up"]);
}

#[test]
fn an_erased_sequence_downcasts_back_or_returns_itself() {
    let erased = AnyPointSequence::from(points("reading", [(10, 1.0_f64)]));
    assert_eq!(erased.label(), "reading");
    assert_eq!(erased.len(), 1);
    assert!(!erased.is_empty());

    let erased = erased
        .downcast::<i64>()
        .expect_err("the values are f64, not i64");
    let typed = erased.downcast::<f64>().expect("the values are f64");

    assert_eq!(typed, points("reading", [(10, 1.0_f64)]));
}

#[test]
fn a_sequence_reports_its_shape_and_size() -> Result<(), IntervalError> {
    let timeline = Sequence::from(TimeIntervalSequence::from_spans(
        "up",
        vec![
            TimeIntervalEvent::span(0, 5)?,
            TimeIntervalEvent::span(10, 15)?,
        ],
    ));
    let readings = Sequence::from(points("reading", [(10, 'a'), (10, 'b'), (20, 'c')]));

    assert_eq!(timeline.len(), 2);
    assert_eq!(readings.len(), 3);
    assert_eq!(readings.attribute().to_string(), "reading");
    assert!(readings.as_time_interval().is_none());
    assert!(timeline.as_time_point::<char>().is_none());
    Ok(())
}

#[test]
fn a_system_can_move_between_threads() {
    fn assert_send_and_sync<T: Send + Sync>() {}

    assert_send_and_sync::<SequenceSystem>();
    assert_send_and_sync::<Batch>();
}

fn described_system() -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert(
        "up",
        TimeIntervalSequence::from_spans("up", vec![span(0, 10)]),
    );
    system.insert(
        "temperature",
        points("temperature", [(2, 1.5_f64), (8, 2.5)]),
    );
    system
        .metadata_mut()
        .insert("unit".to_owned(), "seconds".to_owned());
    system
        .member_metadata_mut("up")
        .expect("up is recorded")
        .insert("sensor".to_owned(), "a".to_owned());
    system
}

#[test]
fn a_member_starts_with_empty_metadata() {
    let system = described_system();
    assert!(system.member_metadata("temperature").unwrap().is_empty());
    assert!(system.member_metadata("missing").is_none());
    assert_eq!(
        system.metadata().get("unit").map(String::as_str),
        Some("seconds")
    );
}

#[test]
fn metadata_survives_clone_slice_and_relabel() {
    let mut system = described_system();
    let cloned = system.clone();
    assert_eq!(cloned, system);

    let sliced = system.slice(0..5);
    assert_eq!(sliced.metadata(), system.metadata());
    assert_eq!(sliced.member_metadata("up"), system.member_metadata("up"));

    system.relabel("up", "uptime").unwrap();
    assert_eq!(
        system
            .member_metadata("up")
            .unwrap()
            .get("sensor")
            .map(String::as_str),
        Some("a")
    );
}

#[test]
fn metadata_takes_part_in_equality() {
    let mut system = described_system();
    let before = system.clone();
    system.metadata_mut().clear();
    assert_ne!(system, before);
}

#[test]
fn remove_then_insert_drops_member_metadata() {
    let mut system = described_system();
    system.remove("up");
    system.insert("up", TimeIntervalSequence::new("up"));
    assert!(system.member_metadata("up").unwrap().is_empty());

    // Replacing in place drops it as well.
    system
        .member_metadata_mut("up")
        .unwrap()
        .insert("k".to_owned(), "v".to_owned());
    system.insert("up", TimeIntervalSequence::new("up"));
    assert!(system.member_metadata("up").unwrap().is_empty());
}

#[test]
fn slicing_a_system_cuts_every_member_and_keeps_labels() {
    let system = described_system();
    let sliced = system.slice(5..);

    let up = sliced.intervals("up").unwrap();
    assert_eq!(up.iter().map(|s| s.bounds()).collect::<Vec<_>>(), [(5, 10)]);
    assert_eq!(up.label(), "up");
    assert!(!up.is_observed(4) && up.is_observed(5));

    let temperature = sliced.points::<f64>("temperature").unwrap();
    assert_eq!(temperature.len(), 1);
    assert_eq!(temperature.label(), "temperature");
}

#[test]
fn slicing_with_unbounded_or_reversed_ranges() {
    let system = described_system();
    assert_eq!(system.slice(..), system);
    assert_eq!(system.slice(..=i64::MAX), system);

    let nothing = system.slice((Bound::Included(7), Bound::Excluded(3)));
    assert!(nothing.intervals("up").unwrap().is_empty());
    assert!(!nothing.intervals("up").unwrap().is_observed(5));
    assert!(nothing.points::<f64>("temperature").unwrap().is_empty());
}

#[test]
fn observing_still_works_with_metadata() {
    let mut system = described_system();
    system.observe(&chronoloom::Extent::from_spans(vec![span(0, 5)]));
    assert_eq!(system.intervals("up").unwrap().active_duration(), 5);
    assert_eq!(system.member_metadata("up").unwrap().len(), 1);
}

// Lineage

fn derived_room() -> SequenceSystem {
    let mut room = SequenceSystem::new();
    room.insert(
        "motion",
        points("motion", [(10, "start"), (25, "end"), (40, "start")]),
    );
    room.insert(
        "door",
        TimeIntervalSequence::from_spans("door", vec![span(0, 5)]),
    );
    Processor::new()
        .with("present", present())
        .apply(&mut room)
        .unwrap();

    room
}

fn present() -> Formula {
    Formula::between(
        Pattern::new("motion", Condition::Equals("start")),
        Pattern::new("motion", Condition::Equals("end")),
    )
}

#[test]
fn apply_and_run_set_the_lineage_and_insert_leaves_none() {
    let room = derived_room();

    assert_eq!(room.lineage("present"), Some(&Output::State(present())));
    assert_eq!(room.lineage("motion"), None);
    assert_eq!(room.lineage("door"), None);
    assert_eq!(room.lineage("missing"), None);

    let produced = Processor::new()
        .with("present", present())
        .with("edges", Events::Edges(Formula::state("door")))
        .run(&room)
        .unwrap();
    assert_eq!(produced.lineage("present"), Some(&Output::State(present())));
    assert_eq!(
        produced.lineage("edges"),
        Some(&Output::Events(Events::Edges(Formula::state("door"))))
    );

    // Inserting under a derived key replaces the member, lineage included.
    let mut replaced = room;
    replaced.insert("present", TimeIntervalSequence::new("present"));
    assert_eq!(replaced.lineage("present"), None);
}

#[test]
fn relabel_slice_clone_and_observe_keep_the_lineage() {
    let mut room = derived_room();
    let expected = Some(Output::State(present()));

    assert_eq!(room.clone().lineage("present").cloned(), expected);
    assert_eq!(room.slice(0..30).lineage("present").cloned(), expected);

    room.relabel("present", "someone is here").unwrap();
    assert_eq!(room.lineage("present").cloned(), expected);

    room.observe(&Extent::from_spans(vec![span(0, 30)]));
    assert_eq!(room.lineage("present").cloned(), expected);
}

#[test]
fn equality_includes_the_lineage() {
    let room = derived_room();
    let mut without = room.clone();
    let present_sequence = without.remove("present").unwrap();
    without.insert("present", present_sequence);

    assert_eq!(without.get("present"), room.get("present"));
    assert_ne!(without, room);
}

#[test]
fn each_recording_path_clears_the_lineage_of_the_member_it_records_into() {
    let mut by_point = derived_room();
    by_point.insert("count", TimePointSequence::<i64>::new("count"));
    Processor::new()
        .with(
            "busy",
            Formula::while_matching(Pattern::new("count", Condition::Greater(3_i64))),
        )
        .apply(&mut by_point)
        .unwrap();
    assert!(by_point.lineage("busy").is_some());

    // Recording into the input leaves the derived member's lineage alone.
    by_point
        .record_point("count", TimePointEvent::new(1, 9_i64))
        .unwrap();
    assert!(by_point.lineage("busy").is_some());

    // Recording into the derived member itself clears it.
    let mut derived_points = SequenceSystem::new();
    derived_points.insert(
        "source",
        TimeIntervalSequence::from_spans("up", vec![span(0, 5)]),
    );
    Processor::new()
        .with("edges", Events::Edges(Formula::state("source")))
        .apply(&mut derived_points)
        .unwrap();
    assert!(derived_points.lineage("edges").is_some());
    derived_points
        .record_point("edges", TimePointEvent::new(9, Edge::Start))
        .unwrap();
    assert_eq!(derived_points.lineage("edges"), None);

    let mut by_span = derived_room();
    by_span.record_span("present", span(50, 60)).unwrap();
    assert_eq!(by_span.lineage("present"), None);

    let mut by_batch = derived_room();
    let mut batch = Batch::new();
    batch.spans("present", [span(50, 60)]);
    by_batch.record_batch(batch).unwrap();
    assert_eq!(by_batch.lineage("present"), None);

    // A refused recording changes nothing, lineage included.
    let mut refused = derived_room();
    assert!(refused.record_span("motion", span(0, 1)).is_err());
    assert!(refused.record_span("present", span(1, 2)).is_ok());
    let mut bad = Batch::new();
    bad.spans("present", [span(3, 4)])
        .spans("missing", [span(3, 4)]);
    let mut again = derived_room();
    assert!(again.record_batch(bad).is_err());
    assert!(again.lineage("present").is_some());
}

#[test]
fn rederive_reads_the_current_system() {
    let mut room = derived_room();
    assert_eq!(room.intervals("present").unwrap()[0].bounds(), (10, 25));
    room.member_metadata_mut("present")
        .unwrap()
        .insert("kept".into(), "yes".into());

    room.record_point("motion", TimePointEvent::new(45, "end"))
        .unwrap();
    // Not yet: the member is as it was until it is derived again.
    assert_eq!(
        room.intervals("present").unwrap()[1].bounds(),
        (40, i64::MAX)
    );

    room.rederive("present").unwrap();
    let derived = room.intervals("present").unwrap();
    assert_eq!(derived.len(), 2);
    assert_eq!(derived[1].bounds(), (40, 45));
    assert!(room.lineage("present").is_some());
    assert_eq!(
        room.member_metadata("present")
            .and_then(|m| m.get("kept"))
            .map(String::as_str),
        Some("yes"),
    );
}

#[test]
fn rederive_refuses_what_it_cannot_derive_and_changes_nothing() {
    let mut room = derived_room();
    let before = room.clone();

    assert_eq!(
        room.rederive("motion"),
        Err(ProcessError::NoLineage {
            key: "motion".into()
        })
    );
    assert_eq!(
        room.rederive("missing"),
        Err(ProcessError::UnknownKey {
            key: "missing".into()
        })
    );

    // Recording cleared the lineage, so there is nothing to derive from.
    room.record_span("present", span(60, 70)).unwrap();
    assert_eq!(
        room.rederive("present"),
        Err(ProcessError::NoLineage {
            key: "present".into()
        })
    );

    // An input that went away is the error evaluation reports.
    let mut orphaned = before.clone();
    orphaned.remove("motion");
    assert_eq!(
        orphaned.rederive("present"),
        Err(ProcessError::UnknownKey {
            key: "motion".into()
        })
    );
    let mut expected = before;
    expected.remove("motion");
    assert_eq!(orphaned, expected);
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

fn reading(distance: i64) -> Value {
    Value::record([
        ("agent", Value::from("a1")),
        ("distance", Value::from(distance)),
    ])
}

fn reading_schema() -> Schema {
    Schema::record([
        ("agent", Schema::Text),
        ("distance", Schema::Integer),
        ("note", Schema::optional(Schema::Text)),
    ])
}

fn wrong_reading() -> Value {
    Value::record([
        ("agent", Value::from("a1")),
        ("distance", Value::from("far")),
    ])
}

/// The error a mismatching `wrong_reading` is.
fn mismatch(key: &str) -> RecordError {
    RecordError::SchemaMismatch {
        key: key.to_owned(),
        error: SchemaError {
            path: "distance".to_owned(),
            expected: "integer".to_owned(),
            found: "text".to_owned(),
        },
    }
}

/// A system with an empty `sensor` of `Value` points under `reading_schema`,
/// beside a `temperature` and an `up`.
fn sensors() -> SequenceSystem {
    let mut system = machine();
    system.insert("sensor", TimePointSequence::<Value>::new("sensor"));
    system.set_schema("sensor", reading_schema()).unwrap();
    system
}

fn point(timestamp: i64, value: Value) -> TimePointEvent<Value> {
    TimePointEvent::new(timestamp, value)
}

#[test]
fn a_member_starts_with_no_schema_and_takes_any_value() {
    let mut system = SequenceSystem::new();
    system.insert("sensor", TimePointSequence::<Value>::new("sensor"));

    assert_eq!(system.schema("sensor"), None);
    assert_eq!(system.schema("missing"), None);
    system
        .record_point("sensor", point(1, wrong_reading()))
        .unwrap();
    system
        .record_point("sensor", point(2, Value::Null))
        .unwrap();
}

#[test]
fn set_schema_files_the_schema_and_replaces_it() {
    let mut system = sensors();
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));

    system.set_schema("sensor", Schema::Any).unwrap();
    assert_eq!(system.schema("sensor"), Some(&Schema::Any));
}

#[test]
fn set_schema_refuses_what_it_cannot_apply_and_changes_nothing() {
    let mut system = sensors();
    let before = system.clone();

    assert_eq!(
        system.set_schema("missing", Schema::Any),
        Err(RecordError::Unknown {
            key: "missing".to_owned()
        })
    );
    assert_eq!(
        system.set_schema("up", Schema::Any),
        Err(RecordError::ShapeMismatch {
            key: "up".to_owned()
        })
    );
    assert!(matches!(
        system.set_schema("temperature", Schema::Any),
        Err(RecordError::ValueTypeMismatch { key, expected, .. })
            if key == "temperature" && expected.contains("f64")
    ));
    assert_eq!(system, before);
}

#[test]
fn set_schema_checks_the_events_already_recorded() {
    let mut system = SequenceSystem::new();
    system.insert(
        "sensor",
        points(
            "sensor",
            [(1, reading(3)), (2, wrong_reading()), (3, reading(4))],
        ),
    );
    let before = system.clone();

    assert_eq!(
        system.set_schema("sensor", reading_schema()),
        Err(mismatch("sensor"))
    );
    assert_eq!(system.schema("sensor"), None);
    assert_eq!(system, before);

    system.set_schema("sensor", Schema::Any).unwrap();
}

#[test]
fn a_mismatching_point_is_refused_naming_the_path_and_changes_nothing() {
    let mut system = sensors();
    system.record_point("sensor", point(1, reading(3))).unwrap();
    let before = system.clone();

    assert_eq!(
        system.record_point("sensor", point(2, wrong_reading())),
        Err(mismatch("sensor"))
    );
    assert_eq!(system, before);

    // An unknown field and a missing one are refused too, with their paths.
    let unknown = Value::record([
        ("agent", Value::from("a1")),
        ("distance", Value::from(1)),
        ("speed", Value::from(2.0)),
    ]);
    assert!(matches!(
        system.record_point("sensor", point(3, unknown)),
        Err(RecordError::SchemaMismatch { error, .. }) if error.path == "speed"
    ));
    assert!(matches!(
        system.record_point("sensor", point(3, Value::record([("agent", Value::from("a1"))]))),
        Err(RecordError::SchemaMismatch { error, .. }) if error.path == "distance"
    ));
    assert_eq!(system, before);

    system.record_point("sensor", point(2, reading(4))).unwrap();
}

#[test]
fn a_batch_with_a_mismatching_value_records_nothing() {
    let mut system = sensors();
    let before = system.clone();

    let mut batch = Batch::new();
    batch
        .points("temperature", [TimePointEvent::new(1, 20.0_f64)])
        .points("sensor", [point(1, reading(3)), point(2, wrong_reading())])
        .spans("up", [span(0, 5)]);
    assert_eq!(system.record_batch(batch), Err(mismatch("sensor")));
    assert_eq!(system, before);

    let mut valid = Batch::new();
    valid
        .points("sensor", [point(2, reading(4)), point(1, reading(3))])
        .points("temperature", [TimePointEvent::new(1, 20.0_f64)]);
    system.record_batch(valid).unwrap();
    assert_eq!(
        system.points::<Value>("sensor").map(TimePointSequence::len),
        Some(2)
    );
}

#[test]
fn a_value_type_mismatch_is_reported_before_the_schema() {
    let mut system = sensors();

    assert!(matches!(
        system.record_point("sensor", TimePointEvent::new(1, 3.5_f64)),
        Err(RecordError::ValueTypeMismatch { .. })
    ));
    let mut batch = Batch::new();
    batch.points("sensor", [TimePointEvent::new(1, 3.5_f64)]);
    assert!(matches!(
        system.record_batch(batch),
        Err(RecordError::ValueTypeMismatch { .. })
    ));
}

#[test]
fn a_live_member_checks_the_schema_after_lateness() {
    let mut system = SequenceSystem::new();
    system.insert_live("sensor", TimePointSequence::<Value>::new("sensor"), 10);
    system.set_schema("sensor", reading_schema()).unwrap();

    // Late, and also mismatching: lateness is reported.
    assert!(matches!(
        system.record_point("sensor", point(5, wrong_reading())),
        Err(RecordError::Late { .. })
    ));
    assert_eq!(
        system.record_point("sensor", point(12, wrong_reading())),
        Err(mismatch("sensor"))
    );
    system
        .record_point("sensor", point(12, reading(3)))
        .unwrap();
}

#[test]
fn the_schema_is_part_of_equality() {
    let with = sensors();
    let mut without = with.clone();
    let sequence = without.remove("sensor").unwrap();
    without.insert("sensor", sequence);

    assert_eq!(without.get("sensor"), with.get("sensor"));
    assert_ne!(without, with);

    let mut other = with.clone();
    other.set_schema("sensor", Schema::Any).unwrap();
    assert_ne!(other, with);
}

#[test]
fn clone_relabel_slice_and_observe_keep_the_schema() {
    let mut system = sensors();
    system.record_point("sensor", point(1, reading(3))).unwrap();
    system
        .record_point("sensor", point(20, reading(4)))
        .unwrap();

    assert_eq!(system.clone().schema("sensor"), Some(&reading_schema()));
    assert_eq!(
        system.slice(0..10).schema("sensor"),
        Some(&reading_schema())
    );

    system.relabel("sensor", "distance sensor").unwrap();
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));

    system.observe(&Extent::from_spans(vec![span(0, 10)]));
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));
    assert_eq!(
        system.record_point("sensor", point(2, wrong_reading())),
        Err(mismatch("sensor"))
    );
}

#[test]
fn draining_keeps_the_schema_on_both_halves() {
    let mut system = SequenceSystem::new();
    system.insert_live("sensor", TimePointSequence::<Value>::new("sensor"), 0);
    system.set_schema("sensor", reading_schema()).unwrap();
    system.record_point("sensor", point(1, reading(3))).unwrap();
    system.record_point("sensor", point(8, reading(4))).unwrap();
    system.advance("sensor", 5).unwrap();

    let drained = system.drain_finalised();

    assert_eq!(
        drained
            .points::<Value>("sensor")
            .map(TimePointSequence::len),
        Some(1)
    );
    assert_eq!(drained.schema("sensor"), Some(&reading_schema()));
    assert_eq!(
        system.points::<Value>("sensor").map(TimePointSequence::len),
        Some(1)
    );
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));
}

#[test]
fn replacing_removing_or_inserting_live_drops_the_schema() {
    let mut replaced = sensors();
    replaced.insert("sensor", TimePointSequence::<Value>::new("sensor"));
    assert_eq!(replaced.schema("sensor"), None);

    let mut removed = sensors();
    removed.remove("sensor");
    removed.insert("sensor", TimePointSequence::<Value>::new("sensor"));
    assert_eq!(removed.schema("sensor"), None);

    let mut live = sensors();
    live.insert_live("sensor", TimePointSequence::<Value>::new("sensor"), 0);
    assert_eq!(live.schema("sensor"), None);
}

#[test]
fn a_derived_member_has_no_schema_and_rederive_keeps_the_one_it_is_given() {
    let mut system = SequenceSystem::new();
    system.insert(
        "sensor",
        points("sensor", [(1, reading(3)), (2, reading(9))]),
    );
    Processor::new()
        .with(
            "far",
            Events::Matching(Pattern::new("sensor", Condition::<Value>::Any)),
        )
        .apply(&mut system)
        .unwrap();
    assert_eq!(system.schema("far"), None);

    system.set_schema("far", reading_schema()).unwrap();
    system.rederive("far").unwrap();
    assert_eq!(system.schema("far"), Some(&reading_schema()));

    // A new reading that does not have the shape reaches the derived member
    // only through the lineage, which refuses it.
    system
        .record_point("sensor", point(3, wrong_reading()))
        .unwrap();
    let before = system.clone();
    assert!(matches!(
        system.rederive("far"),
        Err(ProcessError::SchemaMismatch { key, error })
            if key.as_str() == "far" && error.path == "distance"
    ));
    assert_eq!(system, before);
}
