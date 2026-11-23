//! The live parts of `SequenceSystem` a stream system has not taken over yet:
//! a live member next to a schema.

use chronoloom::primitives::TimePointEvent;
use chronoloom::sequence_system::{RecordError, SequenceSystem};
use chronoloom::sequences::TimePointSequence;
use chronoloom::{Schema, SchemaError, Value};

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

fn point(timestamp: i64, value: Value) -> TimePointEvent<Value> {
    TimePointEvent::new(timestamp, value)
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
fn inserting_live_over_a_schema_drops_it() {
    let mut live = SequenceSystem::new();
    live.insert("sensor", TimePointSequence::<Value>::new("sensor"));
    live.set_schema("sensor", reading_schema()).unwrap();

    live.insert_live("sensor", TimePointSequence::<Value>::new("sensor"), 0);
    assert_eq!(live.schema("sensor"), None);
}
