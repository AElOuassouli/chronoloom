use chronoloom::primitives::TimePointEvent;
use std::collections::BTreeSet;

#[test]
fn new_exposes_timestamp_and_value() {
    let event = TimePointEvent::new(42, 3.5_f64);

    assert_eq!(event.timestamp(), 42);
    assert_eq!(*event.value(), 3.5);
}

#[test]
fn zero_and_negative_timestamps_are_accepted() {
    assert_eq!(TimePointEvent::new(0, ()).timestamp(), 0);
    assert_eq!(TimePointEvent::new(-1, ()).timestamp(), -1);
    assert_eq!(TimePointEvent::new(i64::MIN, ()).timestamp(), i64::MIN);
}

#[test]
fn payload_may_be_a_non_copy_value() {
    let event = TimePointEvent::new(1, String::from("booting"));

    assert_eq!(event.value(), "booting");
    assert_eq!(event.into_value(), "booting");
}

#[test]
fn payload_may_be_a_collection() {
    let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
    let event = TimePointEvent::new(1, tags.clone());

    assert_eq!(event.value(), &tags);
}

#[test]
fn into_parts_round_trips_the_constructor_arguments() {
    let event = TimePointEvent::new(9, 'z');

    assert_eq!(event.into_parts(), (9, 'z'));
}

#[test]
fn map_transforms_the_value_and_preserves_the_timestamp() {
    let event = TimePointEvent::new(9, 2_i32);
    let mapped = event.map(|v| v.to_string());

    assert_eq!(mapped.timestamp(), 9);
    assert_eq!(mapped.value(), "2");
}

#[test]
fn events_compare_on_both_fields() {
    assert_eq!(TimePointEvent::new(1, 'a'), TimePointEvent::new(1, 'a'));
    assert_ne!(TimePointEvent::new(1, 'a'), TimePointEvent::new(2, 'a'));
    assert_ne!(TimePointEvent::new(1, 'a'), TimePointEvent::new(1, 'b'));
}
