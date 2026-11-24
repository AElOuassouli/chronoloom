//! The streams of a stream system: keys, labels, recording, metadata, slicing
//! and schemas.

use std::ops::Bound;

use chronoloom::primitives::{IntervalError, TimeIntervalEvent, TimePointEvent};
use chronoloom::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use chronoloom::stream_system::{AnyPointSequence, Key, RecordError, Sequence};
use chronoloom::stream_system::{StreamSystem, Update, UpdateError};
use chronoloom::{Schema, SchemaError, Value};

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

/// Add `sequence` to `system` as the stream under `key`.
fn insert(system: &mut StreamSystem, key: &str, sequence: impl Into<Sequence>) {
    system
        .insert_stream(key, sequence)
        .expect("a system with no processor takes any stream");
}

/// An update of one push of points.
fn points_update<T: chronoloom::stream_system::PointValue>(
    key: &str,
    events: impl IntoIterator<Item = TimePointEvent<T>>,
) -> Update {
    let mut update = Update::new();
    update.points(key, events);
    update
}

/// A system with an `f64` temperature, a `String` log, and an `up` timeline,
/// all empty.
fn machine() -> StreamSystem {
    let mut machine = StreamSystem::new();
    insert(
        &mut machine,
        "temperature",
        TimePointSequence::<f64>::new("temperature"),
    );
    insert(&mut machine, "log", TimePointSequence::<String>::new("log"));
    insert(&mut machine, "up", TimeIntervalSequence::new("up"));

    machine
}

/// The timestamps the point sequence for `key` holds, in order.
fn timestamps<T: 'static>(system: &StreamSystem, key: &str) -> Vec<i64> {
    system
        .points::<T>(key)
        .expect("the sequence exists with this value type")
        .iter()
        .map(TimePointEvent::timestamp)
        .collect()
}

/// The bounds the timeline for `key` holds, in order.
fn bounds(system: &StreamSystem, key: &str) -> Vec<(i64, i64)> {
    system
        .intervals(key)
        .expect("the timeline exists")
        .iter()
        .map(TimeIntervalEvent::bounds)
        .collect()
}

fn unknown(key: &str) -> UpdateError {
    UpdateError::Record(RecordError::Unknown {
        key: key.to_owned(),
    })
}

fn shape_mismatch(key: &str) -> UpdateError {
    UpdateError::Record(RecordError::ShapeMismatch {
        key: key.to_owned(),
    })
}

#[test]
fn a_new_system_holds_nothing() {
    let system = StreamSystem::new();

    assert!(system.is_empty());
    assert_eq!(system.len(), 0);
    assert_eq!(system.keys().count(), 0);
    assert_eq!(system, StreamSystem::default());
}

#[test]
fn streams_of_both_shapes_and_several_value_types_sit_together() {
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
fn a_stream_is_found_under_the_key_it_was_inserted_with() {
    let a = TimeIntervalSequence::from_spans("A", vec![span(0, 10)]);
    let b = TimeIntervalSequence::from_spans("B", vec![span(20, 30)]);

    let mut system = StreamSystem::new();
    insert(&mut system, "union", a.union(&b));
    insert(
        &mut system,
        "shifted",
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
fn streams_with_one_label_and_different_keys_coexist() {
    let mut system = StreamSystem::new();
    insert(
        &mut system,
        "sensor-1",
        points("temperature", [(10, 1.0_f64)]),
    );
    insert(
        &mut system,
        "sensor-2",
        points("temperature", [(20, 2.0_f64)]),
    );

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
    let mut system = StreamSystem::new();
    insert(&mut system, "b", TimeIntervalSequence::new("a"));
    insert(&mut system, "a", TimeIntervalSequence::new("z"));

    let labels: Vec<(&str, String)> = system
        .keys()
        .map(|key| {
            let sequence = system.get(key.as_str()).expect("the key is listed");
            (key.as_str(), sequence.label())
        })
        .collect();
    assert_eq!(labels, [("a", String::from("z")), ("b", String::from("a"))]);
}

#[test]
fn relabel_changes_the_label_and_keeps_the_key_and_the_data() -> Result<(), UpdateError> {
    let mut system = machine();
    system.update(points_update(
        "temperature",
        [TimePointEvent::new(10, 21.5_f64)],
    ))?;
    let mut spans = Update::new();
    spans.spans("up", [span(0, 10)]);
    system.update(spans)?;

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
        Err(unknown("humidity"))
    );
    assert_eq!(system, before);
}

#[test]
fn equality_compares_keys_and_labels() {
    let mut one = StreamSystem::new();
    insert(&mut one, "x", points("a", [(10, 1)]));
    let mut other_key = StreamSystem::new();
    insert(&mut other_key, "y", points("a", [(10, 1)]));
    let mut other_label = StreamSystem::new();
    insert(&mut other_label, "x", points("b", [(10, 1)]));

    assert_ne!(one, other_key);
    assert_ne!(one, other_label);
    assert_eq!(one, one.clone());
}

#[test]
fn inserting_under_a_recorded_key_replaces_the_stream() {
    let mut system = StreamSystem::new();
    insert(
        &mut system,
        "temperature",
        points("temperature", [(10, 1.0_f64)]),
    );
    insert(
        &mut system,
        "temperature",
        points("temperature", [(20, 2.0_f64)]),
    );

    assert_eq!(system.len(), 1);
    assert_eq!(timestamps::<f64>(&system, "temperature"), [20]);
}

#[test]
fn remove_takes_the_stream_out() {
    let mut system = machine();

    let removed = system
        .remove_stream("up")
        .expect("no processor reads it")
        .expect("the timeline exists");

    assert!(removed.as_time_interval().is_some());
    assert!(!system.contains("up"));
    assert_eq!(system.remove_stream("up"), Ok(None));
    assert_eq!(system.len(), 2);
}

#[test]
fn recorded_points_stay_in_time_order() {
    let mut system = machine();

    for timestamp in [10, 30, 20] {
        system
            .update(points_update(
                "temperature",
                [TimePointEvent::new(timestamp, 0.0_f64)],
            ))
            .expect("the stream holds f64 points");
    }

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 20, 30]);
}

#[test]
fn recorded_spans_merge() -> Result<(), UpdateError> {
    let mut system = machine();

    for (start, end) in [(0, 5), (5, 9), (20, 30)] {
        let mut update = Update::new();
        update.spans("up", [span(start, end)]);
        system.update(update)?;
    }

    assert_eq!(bounds(&system, "up"), [(0, 9), (20, 30)]);
    Ok(())
}

#[test]
fn recording_for_an_unknown_key_creates_nothing() {
    let mut system = machine();
    let before = system.clone();

    let refused = system.update(points_update(
        "humidity",
        [TimePointEvent::new(10, 0.4_f64)],
    ));

    assert_eq!(refused, Err(unknown("humidity")));
    assert_eq!(system, before);
}

#[test]
fn recording_the_wrong_shape_is_refused() {
    let mut system = machine();
    let before = system.clone();

    assert_eq!(
        system.update(points_update("up", [TimePointEvent::new(10, true)])),
        Err(shape_mismatch("up"))
    );
    let mut spans = Update::new();
    spans.spans("temperature", [span(0, 10)]);
    assert_eq!(system.update(spans), Err(shape_mismatch("temperature")));
    assert_eq!(system, before);
}

#[test]
fn recording_the_wrong_value_type_is_refused() {
    let mut system = machine();
    let before = system.clone();

    let refused = system.update(points_update(
        "temperature",
        [TimePointEvent::new(10, 21_i64)],
    ));

    assert_eq!(
        refused,
        Err(UpdateError::Record(RecordError::ValueTypeMismatch {
            key: String::from("temperature"),
            expected: std::any::type_name::<f64>(),
            found: std::any::type_name::<i64>(),
        }))
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
fn an_update_lands_in_the_stream_for_each_attribute() -> Result<(), UpdateError> {
    let mut system = machine();

    let mut update = Update::new();
    update
        .points(
            "temperature",
            [
                TimePointEvent::new(11, 2.0_f64),
                TimePointEvent::new(10, 1.0),
            ],
        )
        .points("log", [TimePointEvent::new(10, String::from("boot"))])
        .spans("up", [span(0, 5), span(5, 10)]);
    assert_eq!(update.len(), 5);

    system.update(update)?;

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 11]);
    assert_eq!(timestamps::<String>(&system, "log"), [10]);
    assert_eq!(bounds(&system, "up"), [(0, 10)]);
    Ok(())
}

#[test]
fn an_update_may_push_to_one_attribute_twice() -> Result<(), UpdateError> {
    let mut system = machine();

    let mut update = Update::new();
    update
        .points("temperature", [TimePointEvent::new(20, 2.0_f64)])
        .points("temperature", [TimePointEvent::new(10, 1.0_f64)]);
    system.update(update)?;

    assert_eq!(timestamps::<f64>(&system, "temperature"), [10, 20]);
    Ok(())
}

#[test]
fn an_empty_update_records_nothing() -> Result<(), UpdateError> {
    let mut system = machine();
    let before = system.clone();

    let update = Update::new();
    assert!(update.is_empty());
    let report = system.update(update)?;

    assert!(report.is_empty());
    assert_eq!(system, before);
    Ok(())
}

#[test]
fn one_bad_push_rejects_the_whole_batch() {
    let bad_pushes: [fn(&mut Update); 3] = [
        |update| {
            update.points("humidity", [TimePointEvent::new(10, 0.4_f64)]);
        },
        |update| {
            update.spans("temperature", [span(0, 10)]);
        },
        |update| {
            update.points("log", [TimePointEvent::new(10, 'x')]);
        },
    ];

    for bad_push in bad_pushes {
        let mut system = machine();
        let before = system.clone();

        let mut update = Update::new();
        update
            .points("temperature", [TimePointEvent::new(10, 21.5_f64)])
            .spans("up", [span(0, 10)]);
        bad_push(&mut update);

        assert!(system.update(update).is_err());
        assert_eq!(system, before);
    }
}

#[test]
fn a_batch_reports_the_first_bad_push() {
    let mut system = machine();

    let mut update = Update::new();
    update
        .spans("temperature", [span(0, 10)])
        .points("humidity", [TimePointEvent::new(10, 0.4_f64)]);

    assert_eq!(system.update(update), Err(shape_mismatch("temperature")));
}

#[test]
fn equality_ignores_insertion_order() {
    let timeline = || TimeIntervalSequence::from_spans("b", vec![span(0, 10)]);

    let mut forward = StreamSystem::new();
    insert(&mut forward, "a", points("a", [(10, 1.0_f64)]));
    insert(&mut forward, "b", timeline());
    let mut backward = StreamSystem::new();
    insert(&mut backward, "b", timeline());
    insert(&mut backward, "a", points("a", [(10, 1.0_f64)]));

    assert_eq!(forward, backward);
}

#[test]
fn same_key_with_another_value_type_is_not_equal() {
    let mut floats = StreamSystem::new();
    insert(
        &mut floats,
        "reading",
        TimePointSequence::<f64>::new("reading"),
    );
    let mut integers = StreamSystem::new();
    insert(
        &mut integers,
        "reading",
        TimePointSequence::<i64>::new("reading"),
    );

    assert_ne!(floats, integers);
}

#[test]
fn a_clone_is_equal_and_independent() -> Result<(), UpdateError> {
    let original = machine();
    let mut copy = original.clone();
    assert_eq!(copy, original);

    copy.update(points_update(
        "temperature",
        [TimePointEvent::new(10, 1.0_f64)],
    ))?;

    assert_ne!(copy, original);
    assert_eq!(
        timestamps::<f64>(&original, "temperature"),
        Vec::<i64>::new()
    );
    Ok(())
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

    assert_send_and_sync::<StreamSystem>();
    assert_send_and_sync::<Update>();
}

fn described_system() -> StreamSystem {
    let mut system = StreamSystem::new();
    insert(
        &mut system,
        "up",
        TimeIntervalSequence::from_spans("up", vec![span(0, 10)]),
    );
    insert(
        &mut system,
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
    system.remove_stream("up").unwrap();
    insert(&mut system, "up", TimeIntervalSequence::new("up"));
    assert!(system.member_metadata("up").unwrap().is_empty());

    // Replacing in place drops it as well.
    system
        .member_metadata_mut("up")
        .unwrap()
        .insert("k".to_owned(), "v".to_owned());
    insert(&mut system, "up", TimeIntervalSequence::new("up"));
    assert!(system.member_metadata("up").unwrap().is_empty());
}

#[test]
fn slicing_a_system_cuts_every_stream_and_keeps_labels() {
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
    system
        .observe(&Extent::from_spans(vec![span(0, 5)]))
        .unwrap();
    assert_eq!(system.intervals("up").unwrap().active_duration(), 5);
    assert_eq!(system.member_metadata("up").unwrap().len(), 1);
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
fn sensors() -> StreamSystem {
    let mut system = machine();
    insert(
        &mut system,
        "sensor",
        TimePointSequence::<Value>::new("sensor"),
    );
    system.set_schema("sensor", reading_schema()).unwrap();
    system
}

fn point(timestamp: i64, value: Value) -> TimePointEvent<Value> {
    TimePointEvent::new(timestamp, value)
}

/// Record one `sensor` point into `system`.
fn record_sensor(
    system: &mut StreamSystem,
    timestamp: i64,
    value: Value,
) -> Result<(), UpdateError> {
    system
        .update(points_update("sensor", [point(timestamp, value)]))
        .map(|_| ())
}

#[test]
fn a_member_starts_with_no_schema_and_takes_any_value() {
    let mut system = StreamSystem::new();
    insert(
        &mut system,
        "sensor",
        TimePointSequence::<Value>::new("sensor"),
    );

    assert_eq!(system.schema("sensor"), None);
    assert_eq!(system.schema("missing"), None);
    record_sensor(&mut system, 1, wrong_reading()).unwrap();
    record_sensor(&mut system, 2, Value::Null).unwrap();
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
        Err(unknown("missing"))
    );
    assert_eq!(
        system.set_schema("up", Schema::Any),
        Err(shape_mismatch("up"))
    );
    assert!(matches!(
        system.set_schema("temperature", Schema::Any),
        Err(UpdateError::Record(RecordError::ValueTypeMismatch { key, expected, .. }))
            if key == "temperature" && expected.contains("f64")
    ));
    assert_eq!(system, before);
}

#[test]
fn set_schema_checks_the_events_already_recorded() {
    let mut system = StreamSystem::new();
    insert(
        &mut system,
        "sensor",
        points(
            "sensor",
            [(1, reading(3)), (2, wrong_reading()), (3, reading(4))],
        ),
    );
    let before = system.clone();

    assert_eq!(
        system.set_schema("sensor", reading_schema()),
        Err(UpdateError::Record(mismatch("sensor")))
    );
    assert_eq!(system.schema("sensor"), None);
    assert_eq!(system, before);

    system.set_schema("sensor", Schema::Any).unwrap();
}

#[test]
fn a_mismatching_point_is_refused_naming_the_path_and_changes_nothing() {
    let mut system = sensors();
    record_sensor(&mut system, 1, reading(3)).unwrap();
    let before = system.clone();

    assert_eq!(
        record_sensor(&mut system, 2, wrong_reading()),
        Err(UpdateError::Record(mismatch("sensor")))
    );
    assert_eq!(system, before);

    // An unknown field and a missing one are refused too, with their paths.
    let unknown_field = Value::record([
        ("agent", Value::from("a1")),
        ("distance", Value::from(1)),
        ("speed", Value::from(2.0)),
    ]);
    assert!(matches!(
        record_sensor(&mut system, 3, unknown_field),
        Err(UpdateError::Record(RecordError::SchemaMismatch { error, .. })) if error.path == "speed"
    ));
    assert!(matches!(
        record_sensor(&mut system, 3, Value::record([("agent", Value::from("a1"))])),
        Err(UpdateError::Record(RecordError::SchemaMismatch { error, .. })) if error.path == "distance"
    ));
    assert_eq!(system, before);

    record_sensor(&mut system, 2, reading(4)).unwrap();
}

#[test]
fn a_batch_with_a_mismatching_value_records_nothing() {
    let mut system = sensors();
    let before = system.clone();

    let mut update = Update::new();
    update
        .points("temperature", [TimePointEvent::new(1, 20.0_f64)])
        .points("sensor", [point(1, reading(3)), point(2, wrong_reading())])
        .spans("up", [span(0, 5)]);
    assert_eq!(
        system.update(update),
        Err(UpdateError::Record(mismatch("sensor")))
    );
    assert_eq!(system, before);

    let mut valid = Update::new();
    valid
        .points("sensor", [point(2, reading(4)), point(1, reading(3))])
        .points("temperature", [TimePointEvent::new(1, 20.0_f64)]);
    system.update(valid).unwrap();
    assert_eq!(
        system.points::<Value>("sensor").map(TimePointSequence::len),
        Some(2)
    );
}

#[test]
fn a_value_type_mismatch_is_reported_before_the_schema() {
    let mut system = sensors();

    let update = points_update("sensor", [TimePointEvent::new(1, 3.5_f64)]);
    assert!(matches!(
        system.update(update),
        Err(UpdateError::Record(RecordError::ValueTypeMismatch { .. }))
    ));
}

#[test]
fn the_schema_is_part_of_equality() {
    let with = sensors();
    let mut without = with.clone();
    let sequence = without.remove_stream("sensor").unwrap().unwrap();
    insert(&mut without, "sensor", sequence);

    assert_eq!(without.get("sensor"), with.get("sensor"));
    assert_ne!(without, with);

    let mut other = with.clone();
    other.set_schema("sensor", Schema::Any).unwrap();
    assert_ne!(other, with);
}

#[test]
fn clone_relabel_slice_and_observe_keep_the_schema() {
    let mut system = sensors();
    record_sensor(&mut system, 1, reading(3)).unwrap();
    record_sensor(&mut system, 20, reading(4)).unwrap();

    assert_eq!(system.clone().schema("sensor"), Some(&reading_schema()));
    assert_eq!(
        system.slice(0..10).schema("sensor"),
        Some(&reading_schema())
    );

    system.relabel("sensor", "distance sensor").unwrap();
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));

    system
        .observe(&Extent::from_spans(vec![span(0, 10)]))
        .unwrap();
    assert_eq!(system.schema("sensor"), Some(&reading_schema()));
    assert_eq!(
        record_sensor(&mut system, 2, wrong_reading()),
        Err(UpdateError::Record(mismatch("sensor")))
    );
}

#[test]
fn replacing_or_removing_and_inserting_a_stream_drops_the_schema() {
    let mut replaced = sensors();
    insert(
        &mut replaced,
        "sensor",
        TimePointSequence::<Value>::new("sensor"),
    );
    assert_eq!(replaced.schema("sensor"), None);

    let mut removed = sensors();
    removed.remove_stream("sensor").unwrap();
    insert(
        &mut removed,
        "sensor",
        TimePointSequence::<Value>::new("sensor"),
    );
    assert_eq!(removed.schema("sensor"), None);
}
