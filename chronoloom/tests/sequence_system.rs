use chronoloom::primitives::{IntervalError, TimeIntervalEvent, TimePointEvent};
use chronoloom::sequence_system::{AnyPointSequence, Batch, RecordError, Sequence, SequenceSystem};
use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};

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
    machine.insert(TimePointSequence::<f64>::new("temperature"));
    machine.insert(TimePointSequence::<String>::new("log"));
    machine.insert(TimeIntervalSequence::new("up"));

    machine
}

/// The timestamps the point sequence for `attribute` holds, in order.
fn timestamps<T: 'static>(system: &SequenceSystem, attribute: &str) -> Vec<i64> {
    system
        .points::<T>(attribute)
        .expect("the sequence exists with this value type")
        .iter()
        .map(TimePointEvent::timestamp)
        .collect()
}

/// The bounds the timeline for `attribute` holds, in order.
fn bounds(system: &SequenceSystem, attribute: &str) -> Vec<(i64, i64)> {
    system
        .intervals(attribute)
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
fn iteration_is_in_attribute_order_however_the_system_was_built() {
    let system = machine();
    let attributes: Vec<&String> = system.attributes().collect();

    assert_eq!(attributes, ["log", "temperature", "up"]);
}

#[test]
fn typed_reads_refuse_the_wrong_type_shape_or_attribute() {
    let system = machine();

    assert!(system.points::<i64>("temperature").is_none());
    assert!(system.points::<f64>("up").is_none());
    assert!(system.intervals("temperature").is_none());
    assert!(system.points::<f64>("humidity").is_none());
    assert!(system.get("humidity").is_none());
}

#[test]
fn a_sequence_is_filed_under_its_own_attribute() {
    let a = TimeIntervalSequence::from_spans("A", vec![span(0, 10)]);
    let b = TimeIntervalSequence::from_spans("B", vec![span(20, 30)]);

    let mut system = SequenceSystem::new();
    system.insert(a.union(&b));
    system.insert(a.transform(1, 2).expect("no overflow"));

    assert!(system.intervals("A ∪ B").is_some());
    assert!(system.intervals("A[1, 2]").is_some());
    for (attribute, sequence) in &system {
        assert_eq!(*attribute, sequence.attribute().to_string());
    }
}

#[test]
fn inserting_for_a_recorded_attribute_returns_what_it_displaced() {
    let mut system = SequenceSystem::new();

    assert!(system
        .insert(points("temperature", [(10, 1.0_f64)]))
        .is_none());
    let displaced = system.insert(points("temperature", [(20, 2.0_f64)]));

    let displaced = displaced.expect("the label was taken");
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
fn recording_for_an_unknown_attribute_creates_nothing() {
    let mut system = machine();
    let before = system.clone();

    let refused = system.record_point("humidity", TimePointEvent::new(10, 0.4_f64));

    assert_eq!(
        refused,
        Err(RecordError::Unknown {
            attribute: String::from("humidity")
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
            attribute: String::from("up")
        })
    );
    assert_eq!(
        system.record_span("temperature", span(0, 10)),
        Err(RecordError::ShapeMismatch {
            attribute: String::from("temperature")
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
            attribute: String::from("temperature"),
            expected: std::any::type_name::<f64>(),
            found: std::any::type_name::<i64>(),
        })
    );
    assert_eq!(system, before);
}

#[test]
fn record_errors_describe_themselves() {
    let unknown = RecordError::Unknown {
        attribute: String::from("humidity"),
    };

    assert_eq!(
        unknown.to_string(),
        "no sequence for `humidity` in the system"
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
            attribute: String::from("temperature")
        })
    );
}

#[test]
fn equality_ignores_insertion_order() {
    let forward: SequenceSystem = [
        Sequence::from(points("a", [(10, 1.0_f64)])),
        Sequence::from(TimeIntervalSequence::from_spans("b", vec![span(0, 10)])),
    ]
    .into_iter()
    .collect();
    let backward: SequenceSystem = [
        Sequence::from(TimeIntervalSequence::from_spans("b", vec![span(0, 10)])),
        Sequence::from(points("a", [(10, 1.0_f64)])),
    ]
    .into_iter()
    .collect();

    assert_eq!(forward, backward);
}

#[test]
fn same_attribute_with_another_value_type_is_not_equal() {
    let floats: SequenceSystem = [TimePointSequence::<f64>::new("reading")]
        .into_iter()
        .collect();
    let integers: SequenceSystem = [TimePointSequence::<i64>::new("reading")]
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
fn collecting_and_extending_keep_the_later_of_two_sequences_for_one_attribute() {
    let mut system: SequenceSystem = [points("reading", [(10, 1)]), points("reading", [(20, 2)])]
        .into_iter()
        .collect();
    assert_eq!(timestamps::<i32>(&system, "reading"), [20]);

    system.extend([points("reading", [(30, 3)])]);
    assert_eq!(timestamps::<i32>(&system, "reading"), [30]);
}

#[test]
fn consuming_iteration_yields_owned_sequences_in_attribute_order() {
    let owned: Vec<(String, Sequence)> = machine().into_iter().collect();

    let attributes: Vec<&str> = owned
        .iter()
        .map(|(attribute, _)| attribute.as_str())
        .collect();
    assert_eq!(attributes, ["log", "temperature", "up"]);
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
