use chronoloom::primitives::{IntervalError, TimeIntervalEvent};
use std::collections::BTreeSet;

#[test]
fn new_exposes_bounds_and_value() {
    let interval = TimeIntervalEvent::new(3, 9, 'x').expect("3 < 9");

    assert_eq!(interval.start(), 3);
    assert_eq!(interval.end(), 9);
    assert_eq!(interval.bounds(), (3, 9));
    assert_eq!(*interval.value(), 'x');
}

#[test]
fn empty_intervals_are_rejected() {
    assert_eq!(
        TimeIntervalEvent::new(5, 5, ()),
        Err(IntervalError::EndNotAfterStart { start: 5, end: 5 })
    );
}

#[test]
fn inverted_intervals_are_rejected() {
    assert_eq!(
        TimeIntervalEvent::new(9, 0, ()),
        Err(IntervalError::EndNotAfterStart { start: 9, end: 0 })
    );
}

#[test]
fn negative_bounds_are_accepted() {
    let interval = TimeIntervalEvent::span(-10, -4).expect("-10 < -4");

    assert_eq!(interval.bounds(), (-10, -4));
    assert_eq!(interval.duration(), 6);
}

#[test]
fn span_builds_a_valueless_interval() {
    let interval = TimeIntervalEvent::span(120, 180).expect("120 < 180");

    assert_eq!(interval.bounds(), (120, 180));
    assert_eq!(interval.into_parts(), (120, 180, ()));
    assert!(TimeIntervalEvent::span(180, 120).is_err());
}

#[test]
fn duration_saturates_at_the_extremes() {
    let widest = TimeIntervalEvent::span(i64::MIN, i64::MAX).expect("MIN < MAX");

    assert_eq!(widest.duration(), i64::MAX);
}

#[test]
fn payload_may_be_a_collection() {
    let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
    let interval = TimeIntervalEvent::new(0, 1, tags.clone()).expect("0 < 1");

    assert_eq!(interval.value(), &tags);
}

#[test]
fn into_parts_round_trips_the_constructor_arguments() {
    let interval = TimeIntervalEvent::new(0, 60, String::from("warm-up")).expect("0 < 60");

    assert_eq!(interval.into_parts(), (0, 60, String::from("warm-up")));
}

#[test]
fn map_transforms_the_value_and_preserves_the_bounds() {
    let interval = TimeIntervalEvent::new(0, 60, 3_i32).expect("0 < 60");
    let mapped = interval.map(|v| v.to_string());

    assert_eq!(mapped.bounds(), (0, 60));
    assert_eq!(mapped.value(), "3");
}

#[test]
fn intervals_compare_on_every_field() {
    let interval = TimeIntervalEvent::new(0, 5, 'a').expect("0 < 5");

    assert_eq!(interval, TimeIntervalEvent::new(0, 5, 'a').expect("0 < 5"));
    assert_ne!(interval, TimeIntervalEvent::new(0, 6, 'a').expect("0 < 6"));
    assert_ne!(interval, TimeIntervalEvent::new(0, 5, 'b').expect("0 < 5"));
}

#[test]
fn error_displays_both_bounds() {
    let error = TimeIntervalEvent::span(9, 0).unwrap_err();

    assert_eq!(
        error.to_string(),
        "interval end (0) must be strictly after its start (9)"
    );
}

#[test]
fn overflow_error_displays_the_bound_and_the_shift() {
    let error = IntervalError::BoundOverflow {
        bound: i64::MAX,
        shift: 1,
    };

    assert_eq!(
        error.to_string(),
        format!(
            "shifting bound ({}) by (1) leaves the timestamp range",
            i64::MAX
        )
    );
}

/// Shorthand for the valueless spans both operations return.
fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).expect("test bounds are ordered")
}

#[test]
fn overlapping_intervals_intersect_on_the_shared_span() {
    let a = span(0, 5);
    let b = span(3, 9);

    assert_eq!(a.intersection(&b), Some(span(3, 5)));
    assert_eq!(b.intersection(&a), Some(span(3, 5)));
}

#[test]
fn a_contained_interval_intersects_to_itself() {
    let outer = span(0, 10);
    let inner = span(3, 5);

    assert_eq!(outer.intersection(&inner), Some(span(3, 5)));
    assert_eq!(inner.intersection(&outer), Some(span(3, 5)));
}

#[test]
fn an_interval_intersects_itself() {
    let a = span(0, 5);

    assert_eq!(a.intersection(&a), Some(span(0, 5)));
}

#[test]
fn touching_intervals_do_not_intersect() {
    let a = span(0, 5);
    let b = span(5, 9);

    assert_eq!(a.intersection(&b), None);
    assert_eq!(b.intersection(&a), None);
}

#[test]
fn disjoint_intervals_do_not_intersect() {
    let a = span(0, 2);
    let b = span(5, 9);

    assert_eq!(a.intersection(&b), None);
    assert_eq!(b.intersection(&a), None);
}

#[test]
fn intersection_handles_negative_bounds() {
    let a = span(-10, -2);
    let b = span(-5, 5);

    assert_eq!(a.intersection(&b), Some(span(-5, -2)));
}

#[test]
fn overlapping_intervals_merge_into_one_span() {
    assert_eq!(span(0, 5).merged(&span(3, 9)), Some(span(0, 9)));
    assert_eq!(span(3, 9).merged(&span(0, 5)), Some(span(0, 9)));
}

#[test]
fn touching_intervals_merge_into_one_span() {
    assert_eq!(span(0, 5).merged(&span(5, 9)), Some(span(0, 9)));
    assert_eq!(span(5, 9).merged(&span(0, 5)), Some(span(0, 9)));
}

#[test]
fn a_contained_interval_merges_into_the_outer_one() {
    assert_eq!(span(0, 10).merged(&span(3, 5)), Some(span(0, 10)));
    assert_eq!(span(3, 5).merged(&span(0, 10)), Some(span(0, 10)));
}

#[test]
fn an_interval_merges_with_itself_into_itself() {
    assert_eq!(span(0, 5).merged(&span(0, 5)), Some(span(0, 5)));
}

#[test]
fn intervals_separated_by_a_gap_do_not_merge() {
    assert_eq!(span(0, 2).merged(&span(5, 9)), None);
    assert_eq!(span(5, 9).merged(&span(0, 2)), None);
}

#[test]
fn merged_handles_negative_bounds() {
    assert_eq!(span(-10, -2).merged(&span(-5, 5)), Some(span(-10, 5)));
}

#[test]
fn merged_agrees_with_union() {
    // `union` is written on top of `merged`; this pins that they stay
    // consistent rather than drifting into two rules.
    for (a, b) in [
        (span(0, 5), span(3, 9)),
        (span(0, 5), span(5, 9)),
        (span(0, 2), span(5, 9)),
        (span(0, 10), span(3, 5)),
    ] {
        match a.merged(&b) {
            Some(merged) => assert_eq!(a.union(&b), vec![merged]),
            None => assert_eq!(a.union(&b).len(), 2),
        }
    }
}

#[test]
fn overlapping_intervals_unite_into_one_span() {
    let a = span(0, 5);
    let b = span(3, 9);

    assert_eq!(a.union(&b), vec![span(0, 9)]);
    assert_eq!(b.union(&a), vec![span(0, 9)]);
}

#[test]
fn touching_intervals_unite_into_one_span() {
    let a = span(0, 5);
    let b = span(5, 9);

    assert_eq!(a.union(&b), vec![span(0, 9)]);
    assert_eq!(b.union(&a), vec![span(0, 9)]);
}

#[test]
fn a_contained_interval_unites_into_the_outer_span() {
    let outer = span(0, 10);
    let inner = span(3, 5);

    assert_eq!(outer.union(&inner), vec![span(0, 10)]);
    assert_eq!(inner.union(&outer), vec![span(0, 10)]);
}

#[test]
fn an_interval_unites_with_itself_into_itself() {
    let a = span(0, 5);

    assert_eq!(a.union(&a), vec![span(0, 5)]);
}

#[test]
fn disjoint_intervals_stay_apart_ordered_by_start() {
    let early = span(0, 2);
    let late = span(5, 9);
    let apart = vec![span(0, 2), span(5, 9)];

    assert_eq!(early.union(&late), apart);
    assert_eq!(late.union(&early), apart);
}

#[test]
fn union_yields_one_span_or_two_and_never_more() {
    let a = span(0, 5);

    assert_eq!(a.union(&span(3, 9)).len(), 1);
    assert_eq!(a.union(&span(5, 9)).len(), 1);
    assert_eq!(a.union(&span(20, 30)).len(), 2);
}

#[test]
fn union_handles_negative_bounds() {
    let a = span(-10, -5);
    let b = span(-5, 0);

    assert_eq!(a.union(&b), vec![span(-10, 0)]);
}

#[test]
fn operations_combine_intervals_carrying_different_value_types() {
    let measured = TimeIntervalEvent::new(0, 5, 21.5_f64).expect("0 < 5");
    let labelled = TimeIntervalEvent::new(3, 9, String::from("warm-up")).expect("3 < 9");

    assert_eq!(measured.intersection(&labelled), Some(span(3, 5)));
    assert_eq!(measured.union(&labelled), vec![span(0, 9)]);

    assert_eq!(measured.value(), &21.5);
    assert_eq!(labelled.value(), "warm-up");
}

#[test]
fn error_is_a_standard_error() {
    let error: Box<dyn std::error::Error> = Box::new(TimeIntervalEvent::span(5, 5).unwrap_err());

    assert!(error.to_string().contains("strictly after"));
}
