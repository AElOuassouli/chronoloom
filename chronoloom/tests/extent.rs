//! Extents, and how sequences and systems keep their data inside one.

use chronoloom::{
    Extent, SequenceSystem, TimeIntervalEvent, TimeIntervalSequence, TimePointEvent,
    TimePointSequence,
};

fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).unwrap()
}

fn bounds(timeline: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    timeline.iter().map(TimeIntervalEvent::bounds).collect()
}

fn window(start: i64, end: i64) -> Extent {
    Extent::from_spans(vec![span(start, end)])
}

fn timeline(spans: &[(i64, i64)]) -> TimeIntervalSequence {
    TimeIntervalSequence::from_spans("a", spans.iter().map(|&(s, e)| span(s, e)).collect())
}

#[test]
fn everything_is_the_default_and_excludes_only_the_last_instant() {
    let everything = Extent::everything();
    assert_eq!(Extent::default(), everything);
    assert!(everything.contains(i64::MIN));
    assert!(everything.contains(0));
    assert!(everything.contains(i64::MAX - 1));
    assert!(!everything.contains(i64::MAX));
}

#[test]
fn nothing_observes_no_instant() {
    assert!(!Extent::nothing().contains(0));
    assert!(Extent::nothing().as_slice().is_empty());
}

#[test]
fn from_spans_normalizes() {
    let extent = Extent::from_spans(vec![span(10, 20), span(0, 5), span(5, 12)]);
    assert_eq!(extent.as_slice(), [span(0, 20)]);
    assert!(extent.contains(0) && extent.contains(19) && !extent.contains(20));
}

#[test]
fn a_new_timeline_is_observed_everywhere() {
    let up = timeline(&[(0, 5)]);
    assert_eq!(up.observed(), &Extent::everything());
    assert!(up.is_observed(1_000_000));
}

#[test]
fn with_observed_clips_a_timeline() {
    let up = timeline(&[(0, 10), (20, 30)]).with_observed(window(5, 25));
    assert_eq!(bounds(&up), [(5, 10), (20, 25)]);
    assert_eq!(up.active_duration(), 10);
    assert!(up.is_observed(7) && !up.is_observed(4));
}

#[test]
fn with_observed_drops_points_outside() {
    let readings = TimePointSequence::from_events(
        "r",
        vec![
            TimePointEvent::new(1, 'a'),
            TimePointEvent::new(7, 'b'),
            TimePointEvent::new(9, 'c'),
        ],
    )
    .with_observed(window(5, 9));
    let kept: Vec<i64> = readings.iter().map(TimePointEvent::timestamp).collect();
    assert_eq!(kept, [7]);
    assert!(readings.is_observed(8) && !readings.is_observed(9));
}

#[test]
fn inserting_a_span_outside_the_extent_grows_it() {
    let mut up = timeline(&[]).with_observed(window(0, 10));
    up.insert(span(8, 15));
    up.insert(span(30, 40));
    assert_eq!(up.observed().as_slice(), [span(0, 15), span(30, 40)]);
    assert_eq!(bounds(&up), [(8, 15), (30, 40)]);
}

#[test]
fn extending_a_timeline_grows_the_extent() {
    let mut up = timeline(&[]).with_observed(window(0, 10));
    up.extend([span(5, 12), span(20, 21)]);
    assert_eq!(up.observed().as_slice(), [span(0, 12), span(20, 21)]);
}

#[test]
fn inserting_a_point_outside_the_extent_grows_it() {
    let mut readings = TimePointSequence::<u8>::new("r").with_observed(window(0, 10));
    readings.insert(TimePointEvent::new(10, 1));
    readings.insert(TimePointEvent::new(50, 2));
    readings.extend([TimePointEvent::new(3, 3), TimePointEvent::new(51, 4)]);
    assert_eq!(readings.observed().as_slice(), [span(0, 11), span(50, 52)]);
}

#[test]
fn default_extents_never_grow_or_change_equality() {
    let mut readings = TimePointSequence::<u8>::new("r");
    readings.insert(TimePointEvent::new(i64::MAX, 1));
    readings.insert(TimePointEvent::new(i64::MIN, 2));
    assert_eq!(readings.observed(), &Extent::everything());
    assert_eq!(readings.len(), 2);

    let mut up = timeline(&[]);
    up.insert(TimeIntervalEvent::span(i64::MIN, i64::MAX).unwrap());
    assert_eq!(up.observed(), &Extent::everything());
}

#[test]
fn equality_includes_the_extent_but_covers_same_does_not() {
    let whole = timeline(&[(0, 5)]);
    let partial = timeline(&[(0, 5)]).with_observed(window(0, 10));
    assert_ne!(whole, partial);
    assert!(whole.covers_same(&partial));
}

#[test]
fn complement_flips_within_the_extent() {
    let up = timeline(&[(2, 4)]).with_observed(window(0, 6));
    let down = up.complement();
    assert_eq!(bounds(&down), [(0, 2), (4, 6)]);
    assert_eq!(down.observed(), up.observed());
    assert_eq!(down.to_string(), "¬a");
}

#[test]
fn complement_of_everything_stays_inside_the_range() {
    let down = timeline(&[(0, 5)]).complement();
    assert_eq!(bounds(&down), [(i64::MIN, 0), (5, i64::MAX)]);
}

#[test]
fn complement_labels_group_where_needed() {
    let a = timeline(&[(0, 1)]).with_observed(window(-100, 100));
    let b = TimeIntervalSequence::new("b");
    assert_eq!(a.union(&b).complement().to_string(), "¬(a ∪ b)");
    assert_eq!(a.complement().union(&b).to_string(), "¬a ∪ b");
    assert_eq!(
        a.complement().transform(1, 2).unwrap().to_string(),
        "(¬a)[1, 2]"
    );
    assert_eq!(
        a.transform(1, 2).unwrap().complement().to_string(),
        "¬a[1, 2]"
    );
}

#[test]
fn a_union_is_active_where_one_side_is_known_active() {
    let seen = timeline(&[(0, 5)]).with_observed(window(0, 10));
    let blind = TimeIntervalSequence::new("b").with_observed(Extent::nothing());
    let either = seen.union(&blind);
    assert_eq!(bounds(&either), [(0, 5)]);
    assert!(either.is_observed(2));
    assert!(!either.is_observed(7));
}

#[test]
fn a_transform_that_narrows_decides_inactive_beside_an_inactive_instant() {
    // Observed only at [0, 5); the rest is unknown. A window of three that
    // touches the known-inactive instant 0 is inactive without more.
    let up = timeline(&[]).with_observed(window(0, 5));
    let narrowed = up.transform(2, 0).unwrap();
    assert!(narrowed.is_observed(0));
    assert!(narrowed.is_observed(2));
    assert!(narrowed.is_observed(6));
    assert!(!narrowed.is_observed(8));
}

#[test]
fn a_system_observe_narrows_every_member() {
    let mut system = SequenceSystem::new();
    system.insert("a", timeline(&[(0, 50)]));
    system.insert(
        "r",
        TimePointSequence::from_events(
            "r",
            vec![
                TimePointEvent::new(5, 1.0_f64),
                TimePointEvent::new(30, 2.0),
            ],
        ),
    );

    system.observe(&window(0, 20));

    let up = system.get("a").unwrap().as_time_interval().unwrap();
    assert_eq!(bounds(up), [(0, 20)]);
    let readings = system.points::<f64>("r").unwrap();
    assert_eq!(readings.len(), 1);
    assert_eq!(system.get("r").unwrap().observed(), &window(0, 20));

    // Narrowing only ever narrows.
    system.observe(&window(10, 100));
    assert_eq!(system.get("a").unwrap().observed(), &window(10, 20));
}
