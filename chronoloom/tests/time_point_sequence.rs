use chronoloom::primitives::TimePointEvent;
use chronoloom::sequences::TimePointSequence;
use std::collections::BTreeSet;
use std::ops::Bound;

/// Build a sequence by inserting `(timestamp, value)` pairs one at a time,
/// in the order given — the path `FromIterator` does not take.
fn inserted<T>(events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
    let mut sequence = TimePointSequence::new();
    for (timestamp, value) in events {
        sequence.insert(TimePointEvent::new(timestamp, value));
    }

    sequence
}

/// Build a sequence from `(timestamp, value)` pairs through `collect`.
fn collected<T>(events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
    events
        .into_iter()
        .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
        .collect()
}

/// Build a sequence from `(timestamp, value)` pairs through `from_events`.
fn from_events<T>(events: impl IntoIterator<Item = (i64, T)>) -> TimePointSequence<T> {
    TimePointSequence::from_events(
        events
            .into_iter()
            .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
            .collect(),
    )
}

/// The timestamps a sequence reads, in order.
fn timestamps<T>(sequence: &TimePointSequence<T>) -> Vec<i64> {
    sequence.iter().map(TimePointEvent::timestamp).collect()
}

/// The values a sequence reads, in order.
fn values<T: Copy>(sequence: &TimePointSequence<T>) -> Vec<T> {
    sequence.iter().map(|event| *event.value()).collect()
}

#[test]
fn events_read_in_time_order_however_they_arrive() {
    assert_eq!(
        timestamps(&inserted([(30, 'c'), (10, 'a'), (20, 'b')])),
        [10, 20, 30]
    );
    assert_eq!(
        timestamps(&collected([(30, 'c'), (10, 'a'), (20, 'b')])),
        [10, 20, 30]
    );
}

#[test]
fn every_construction_path_agrees() {
    let events = [(30, 'c'), (10, 'a'), (20, 'b'), (10, 'z'), (-5, 'y')];

    assert_eq!(inserted(events), collected(events));
    assert_eq!(from_events(events), collected(events));
}

#[test]
fn from_events_orders_whatever_it_is_given() {
    assert_eq!(
        timestamps(&from_events([(30, 'c'), (10, 'a'), (20, 'b')])),
        [10, 20, 30]
    );
}

#[test]
fn from_events_leaves_already_ordered_input_alone() {
    let readings = from_events([(10, 'a'), (20, 'b'), (30, 'c')]);

    assert_eq!(values(&readings), ['a', 'b', 'c']);
}

#[test]
fn from_events_keeps_given_order_within_an_instant() {
    assert_eq!(
        values(&from_events([(20, 'd'), (10, 'c'), (10, 'a')])),
        ['c', 'a', 'd']
    );
}

#[test]
fn from_events_accepts_nothing() {
    let readings: TimePointSequence<char> = TimePointSequence::from_events(vec![]);

    assert!(readings.is_empty());
    assert_eq!(readings, TimePointSequence::new());
}

#[test]
fn into_events_round_trips_from_events() {
    let readings = from_events([(30, 'c'), (10, 'a'), (20, 'b')]);
    let events = readings.clone().into_events();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].timestamp(), 10);
    assert_eq!(TimePointSequence::from_events(events), readings);
}

#[test]
fn negative_timestamps_sort_before_positive_ones() {
    assert_eq!(
        timestamps(&inserted([(5, 'c'), (-10, 'a'), (0, 'b')])),
        [-10, 0, 5]
    );
}

#[test]
fn several_events_may_share_an_instant() {
    let readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);

    assert_eq!(readings.len(), 3);
    assert_eq!(readings.instant_count(), 2);
    assert_eq!(readings.get(10).len(), 2);
    assert_eq!(timestamps(&readings), [10, 10, 20]);
}

#[test]
fn events_sharing_an_instant_keep_insertion_order() {
    assert_eq!(
        values(&inserted([(10, 'c'), (10, 'a'), (10, 'b')])),
        ['c', 'a', 'b']
    );
}

#[test]
fn an_out_of_order_insert_lands_after_events_sharing_its_instant() {
    // 'z' arrives last but belongs at instant 10, after 'a'.
    let readings = inserted([(10, 'a'), (30, 'c'), (10, 'z')]);

    assert_eq!(values(&readings), ['a', 'z', 'c']);
}

#[test]
fn collecting_keeps_source_order_within_an_instant() {
    assert_eq!(
        values(&collected([(10, 'c'), (20, 'd'), (10, 'a')])),
        ['c', 'a', 'd']
    );
}

#[test]
fn remove_takes_every_event_at_the_instant() {
    let mut readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);

    let removed: Vec<char> = readings
        .remove(10)
        .into_iter()
        .map(TimePointEvent::into_value)
        .collect();

    assert_eq!(removed, ['a', 'b']);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings.instant_count(), 1);
    assert!(!readings.contains(10));
}

#[test]
fn remove_keeps_the_surrounding_events_in_order() {
    let mut readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);
    readings.remove(20);

    assert_eq!(timestamps(&readings), [10, 30]);
}

#[test]
fn removing_an_absent_instant_changes_nothing() {
    let mut readings = inserted([(10, 'a')]);

    assert!(readings.remove(99).is_empty());
    assert!(readings.remove(-99).is_empty());
    assert_eq!(readings.len(), 1);
}

#[test]
fn len_stays_exact_across_interleaved_mutations() {
    let mut readings = inserted([(10, 'a'), (10, 'b'), (20, 'c')]);
    assert_eq!(readings.len(), 3);

    readings.remove(10);
    assert_eq!(readings.len(), 1);

    readings.extend([TimePointEvent::new(30, 'd'), TimePointEvent::new(30, 'e')]);
    assert_eq!(readings.len(), 3);

    readings.insert(TimePointEvent::new(20, 'f'));
    assert_eq!(readings.len(), 4);
    assert_eq!(timestamps(&readings), [20, 20, 30, 30]);

    readings.clear();
    assert_eq!(readings.len(), 0);
    assert_eq!(readings.instant_count(), 0);
    assert!(readings.is_empty());
}

#[test]
fn len_counts_events_while_instant_count_counts_instants() {
    let readings = inserted([(10, 'a'), (10, 'b'), (10, 'c')]);

    assert_eq!(readings.len(), 3);
    assert_eq!(readings.instant_count(), 1);
}

#[test]
fn get_is_empty_for_an_instant_that_holds_nothing() {
    let readings = inserted([(10, 'a'), (30, 'b')]);

    assert!(readings.get(20).is_empty());
    assert!(readings.get(0).is_empty());
    assert!(readings.get(99).is_empty());
    assert!(!readings.contains(20));
}

#[test]
fn as_slice_exposes_every_event_in_order() {
    let readings = inserted([(20, 'b'), (10, 'a')]);

    assert_eq!(readings.as_slice().len(), 2);
    assert_eq!(readings.as_slice()[0].timestamp(), 10);
}

#[test]
fn nth_and_indexing_read_by_position() {
    let readings = inserted([(20, 'b'), (10, 'a')]);

    assert_eq!(readings.nth(0).map(TimePointEvent::timestamp), Some(10));
    assert_eq!(readings.nth(1).map(TimePointEvent::timestamp), Some(20));
    assert!(readings.nth(2).is_none());
    assert_eq!(readings[1].timestamp(), 20);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn indexing_past_the_end_panics() {
    let readings = inserted([(10, 'a')]);

    let _ = readings[5];
}

#[test]
fn range_excludes_its_end_and_includes_its_start() {
    let readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);

    let window: Vec<i64> = readings
        .range(10..30)
        .iter()
        .map(TimePointEvent::timestamp)
        .collect();
    assert_eq!(window, [10, 20]);
}

#[test]
fn range_honours_inclusive_and_unbounded_ends() {
    let readings = inserted([(10, 'a'), (20, 'b'), (30, 'c')]);

    assert_eq!(readings.range(10..=30).len(), 3);
    assert_eq!(readings.range(20..).len(), 2);
    assert_eq!(readings.range(..20).len(), 1);
    assert_eq!(readings.range(..).len(), 3);
}

#[test]
fn range_between_occupied_instants_yields_nothing() {
    let readings = inserted([(10, 'a'), (30, 'b')]);

    assert!(readings.range(15..25).is_empty());
    assert!(readings.range(100..200).is_empty());
    assert!(readings.range(-200..-100).is_empty());
}

#[test]
fn range_yields_every_event_at_a_shared_instant() {
    let readings = inserted([(10, 'a'), (20, 'b'), (20, 'c'), (30, 'd')]);

    let window: Vec<char> = readings.range(20..30).iter().map(|e| *e.value()).collect();
    assert_eq!(window, ['b', 'c']);
}

#[test]
#[should_panic(expected = "range start is greater than range end")]
fn an_inverted_range_panics() {
    let readings = inserted([(10, 'a'), (30, 'b')]);

    // Spelled with explicit bounds because clippy rejects the literal
    // `30..10` outright.
    let _ = readings.range((Bound::Included(30), Bound::Excluded(10)));
}

#[test]
fn first_and_last_bracket_the_sequence() {
    let readings = inserted([(20, 'b'), (10, 'a'), (30, 'c')]);

    assert_eq!(readings.first().map(TimePointEvent::timestamp), Some(10));
    assert_eq!(readings.last().map(TimePointEvent::timestamp), Some(30));
}

#[test]
fn first_and_last_pick_the_right_end_of_a_shared_instant() {
    let readings = inserted([(10, 'a'), (10, 'b')]);

    assert_eq!(readings.first().map(|e| *e.value()), Some('a'));
    assert_eq!(readings.last().map(|e| *e.value()), Some('b'));
}

#[test]
fn before_and_after_include_an_exact_match() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    assert_eq!(readings.before(20).map(TimePointEvent::timestamp), Some(20));
    assert_eq!(readings.after(10).map(TimePointEvent::timestamp), Some(10));
}

#[test]
fn before_and_after_step_to_the_neighbouring_instant() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    assert_eq!(readings.before(15).map(TimePointEvent::timestamp), Some(10));
    assert_eq!(readings.after(15).map(TimePointEvent::timestamp), Some(20));
}

#[test]
fn before_and_after_pick_the_right_end_of_a_shared_instant() {
    let readings = inserted([(10, 'a'), (10, 'b')]);

    assert_eq!(readings.before(10).map(|e| *e.value()), Some('b'));
    assert_eq!(readings.after(10).map(|e| *e.value()), Some('a'));
}

#[test]
fn before_and_after_run_out_past_the_edges() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    assert!(readings.before(5).is_none());
    assert!(readings.after(25).is_none());
}

#[test]
fn nearest_picks_the_closer_side() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    assert_eq!(
        readings.nearest(12).map(TimePointEvent::timestamp),
        Some(10)
    );
    assert_eq!(
        readings.nearest(18).map(TimePointEvent::timestamp),
        Some(20)
    );
}

#[test]
fn nearest_breaks_a_tie_toward_the_earlier_event() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    assert_eq!(
        readings.nearest(15).map(TimePointEvent::timestamp),
        Some(10)
    );
}

#[test]
fn nearest_falls_back_to_whichever_side_exists() {
    let readings = inserted([(10, 'a')]);

    assert_eq!(readings.nearest(5).map(TimePointEvent::timestamp), Some(10));
    assert_eq!(
        readings.nearest(50).map(TimePointEvent::timestamp),
        Some(10)
    );
}

#[test]
fn nearest_survives_the_timestamp_extremes() {
    let readings = inserted([(i64::MIN, 'a'), (i64::MAX, 'b')]);

    assert_eq!(
        readings.nearest(-1).map(TimePointEvent::timestamp),
        Some(i64::MIN)
    );
    assert_eq!(
        readings.nearest(1).map(TimePointEvent::timestamp),
        Some(i64::MAX)
    );
}

#[test]
fn an_empty_sequence_answers_nothing() {
    let readings: TimePointSequence<char> = TimePointSequence::new();

    assert!(readings.is_empty());
    assert_eq!(readings.len(), 0);
    assert_eq!(readings.instant_count(), 0);
    assert!(readings.get(10).is_empty());
    assert!(readings.as_slice().is_empty());
    assert!(readings.range(..).is_empty());
    assert!(readings.nth(0).is_none());
    assert!(readings.first().is_none());
    assert!(readings.last().is_none());
    assert!(readings.before(10).is_none());
    assert!(readings.after(10).is_none());
    assert!(readings.nearest(10).is_none());
    assert_eq!(readings.iter().count(), 0);
}

#[test]
fn new_and_default_agree() {
    let built: TimePointSequence<char> = TimePointSequence::new();

    assert_eq!(built, TimePointSequence::default());
}

#[test]
fn collecting_round_trips_through_into_iter() {
    let readings = collected([(30, 'c'), (10, 'a'), (20, 'b'), (10, 'z')]);

    let owned: Vec<(i64, char)> = readings
        .clone()
        .into_iter()
        .map(TimePointEvent::into_parts)
        .collect();
    assert_eq!(owned, [(10, 'a'), (10, 'z'), (20, 'b'), (30, 'c')]);

    let rebuilt: TimePointSequence<char> = owned
        .into_iter()
        .map(|(timestamp, value)| TimePointEvent::new(timestamp, value))
        .collect();
    assert_eq!(rebuilt, readings);
}

#[test]
fn extend_merges_onto_occupied_instants() {
    let mut readings = inserted([(10, 'a')]);
    readings.extend([TimePointEvent::new(10, 'b'), TimePointEvent::new(20, 'c')]);

    assert_eq!(readings.len(), 3);
    assert_eq!(values(&readings), ['a', 'b', 'c']);
}

#[test]
fn extend_reorders_when_the_additions_arrive_out_of_order() {
    let mut readings = inserted([(20, 'b')]);
    readings.extend([TimePointEvent::new(30, 'c'), TimePointEvent::new(10, 'a')]);

    assert_eq!(timestamps(&readings), [10, 20, 30]);
}

#[test]
fn extend_onto_an_empty_sequence_still_orders() {
    let mut readings = TimePointSequence::new();
    readings.extend([TimePointEvent::new(20, 'b'), TimePointEvent::new(10, 'a')]);

    assert_eq!(timestamps(&readings), [10, 20]);
}

#[test]
fn borrowed_iteration_leaves_the_sequence_usable() {
    let readings = inserted([(10, 'a'), (20, 'b')]);

    let borrowed: Vec<i64> = (&readings)
        .into_iter()
        .map(TimePointEvent::timestamp)
        .collect();

    assert_eq!(borrowed, [10, 20]);
    assert_eq!(readings.len(), 2);
}

#[test]
fn payload_may_be_a_non_copy_value() {
    let readings = inserted([(10, String::from("start")), (20, String::from("stop"))]);

    assert_eq!(readings.first().map(|e| e.value().as_str()), Some("start"));
    assert_eq!(readings.get(20)[0].value(), "stop");
}

#[test]
fn payload_may_be_a_collection() {
    let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
    let readings = inserted([(10, tags.clone())]);

    assert_eq!(readings.get(10)[0].value(), &tags);
}
