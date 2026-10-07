use chronoloom::primitives::{IntervalError, TimeIntervalEvent};
use chronoloom::sequences::{Attribute, TimeIntervalSequence};

/// A pair of operands, each as its `(start, end)` bounds.
type OperandShapes<'a> = (&'a [(i64, i64)], &'a [(i64, i64)]);

/// A span, for tests where the bounds are known good.
fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).expect("test bounds are ordered")
}

/// The state the helpers below build a timeline of.
///
/// Shared, rather than named per test, because equality now asks what a
/// timeline describes as well as which instants it covers: two timelines
/// built by two different helpers must still be able to come out equal.
/// Tests that are *about* the description name their own states instead.
const ATTRIBUTE: &str = "up";

/// Build a timeline of [`ATTRIBUTE`] from `(start, end)` pairs through
/// `from_spans`.
fn from_spans(bounds: impl IntoIterator<Item = (i64, i64)>) -> TimeIntervalSequence {
    let sequence = TimeIntervalSequence::from_spans(
        ATTRIBUTE,
        bounds
            .into_iter()
            .map(|(start, end)| span(start, end))
            .collect(),
    );
    assert_normalized(&sequence);

    sequence
}

/// Build a timeline of [`ATTRIBUTE`] by inserting `(start, end)` pairs one
/// at a time, checking the invariant holds after every step.
fn inserted(bounds: impl IntoIterator<Item = (i64, i64)>) -> TimeIntervalSequence {
    let mut sequence = TimeIntervalSequence::new(ATTRIBUTE);
    for (start, end) in bounds {
        sequence.insert(span(start, end));
        assert_normalized(&sequence);
    }

    sequence
}

/// The bounds a sequence reads, in order.
fn bounds(sequence: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    sequence.iter().map(TimeIntervalEvent::bounds).collect()
}

/// Assert an operation means what it claims, one instant at a time.
///
/// Rather than trusting a hand-written list of expected spans — which can
/// encode the very off-by-one it is meant to catch — this checks that the
/// result covers exactly the instants the boolean rule says it should,
/// across the whole range the operands touch and a margin either side.
fn assert_covers(
    result: &TimeIntervalSequence,
    a: &TimeIntervalSequence,
    b: &TimeIntervalSequence,
    rule: impl Fn(bool, bool) -> bool,
) {
    assert_normalized(result);

    for timestamp in -5..=105 {
        assert_eq!(
            result.contains(timestamp),
            rule(a.contains(timestamp), b.contains(timestamp)),
            "instant {timestamp} is on the wrong side of the result",
        );
    }
}

/// Transform a timeline by shifts known to stay in range.
fn transformed(sequence: &TimeIntervalSequence, alpha: i64, beta: i64) -> TimeIntervalSequence {
    let result = sequence
        .transform(alpha, beta)
        .expect("test shifts stay inside the timestamp range");
    assert_normalized(&result);

    result
}

/// Assert two timelines cover the same instants, whatever they describe.
///
/// What the set-algebra identities are stated with: `A ∪ B` and `B ∪ A`
/// cover the same instants, but describe themselves differently, so full
/// equality is the wrong question to ask of them.
fn assert_same_coverage(left: &TimeIntervalSequence, right: &TimeIntervalSequence) {
    assert!(
        left.covers_same(right),
        "{left} covers {:?}, but {right} covers {:?}",
        bounds(left),
        bounds(right),
    );
}

/// Every operation, checked against its boolean rule on the same operands.
fn assert_all_operations(a: &TimeIntervalSequence, b: &TimeIntervalSequence) {
    assert_covers(&a.union(b), a, b, |x, y| x || y);
    assert_covers(&a.intersection(b), a, b, |x, y| x && y);
    assert_covers(&a.difference(b), a, b, |x, y| x && !y);
    assert_covers(&a.symmetric_difference(b), a, b, |x, y| x ^ y);
}

/// The invariants every method must restore: sorted, disjoint, with no two
/// spans left touching, and a duration that still totals what is there.
///
/// The duration check lives here, rather than in tests of its own, because
/// every builder and every operation result already passes through this —
/// so a mutation path that forgets to update the cache fails the whole
/// suite rather than only the tests that thought to look.
fn assert_normalized(sequence: &TimeIntervalSequence) {
    for pair in sequence.as_slice().windows(2) {
        assert!(
            pair[0].end() < pair[1].start(),
            "spans {:?} and {:?} should have merged",
            pair[0].bounds(),
            pair[1].bounds(),
        );
    }

    // The crate's own `span_ticks` is private, so recompute it here rather than
    // widen the API for a test. Via `i128` for the same reason it does: the
    // widest span in range is `i64::MAX - i64::MIN` ticks, which overflows
    // `i64` but is exactly `u64::MAX`.
    let recomputed: u64 = sequence
        .iter()
        .map(|span| {
            let (start, end) = span.bounds();
            u64::try_from(i128::from(end) - i128::from(start))
                .expect("a span always ends after it starts")
        })
        .sum();
    assert_eq!(
        sequence.active_duration(),
        recomputed,
        "active_duration disagrees with the spans {:?}",
        bounds(sequence),
    );
}

#[test]
fn disjoint_spans_are_all_kept_in_order() {
    assert_eq!(bounds(&inserted([(30, 40), (0, 10)])), [(0, 10), (30, 40)]);
    assert_eq!(
        bounds(&from_spans([(30, 40), (0, 10)])),
        [(0, 10), (30, 40)]
    );
}

#[test]
fn overlapping_spans_merge() {
    assert_eq!(bounds(&inserted([(0, 10), (5, 20)])), [(0, 20)]);
    assert_eq!(bounds(&from_spans([(0, 10), (5, 20)])), [(0, 20)]);
}

#[test]
fn touching_spans_merge() {
    // No instant separates [0, 5) from [5, 9), so they cover exactly [0, 9).
    assert_eq!(bounds(&inserted([(0, 5), (5, 9)])), [(0, 9)]);
    assert_eq!(bounds(&inserted([(5, 9), (0, 5)])), [(0, 9)]);
    assert_eq!(bounds(&from_spans([(5, 9), (0, 5)])), [(0, 9)]);
}

#[test]
fn a_span_barely_short_of_touching_stays_separate() {
    assert_eq!(bounds(&inserted([(0, 5), (6, 9)])), [(0, 5), (6, 9)]);
}

#[test]
fn overlap_on_either_side_extends_the_existing_span() {
    assert_eq!(bounds(&inserted([(10, 20), (15, 30)])), [(10, 30)]);
    assert_eq!(bounds(&inserted([(10, 20), (0, 15)])), [(0, 20)]);
}

#[test]
fn a_contained_span_changes_nothing() {
    let uptime = inserted([(0, 20), (5, 10)]);

    assert_eq!(bounds(&uptime), [(0, 20)]);
    assert_eq!(uptime.len(), 1);
}

#[test]
fn an_identical_span_changes_nothing() {
    assert_eq!(bounds(&inserted([(0, 20), (0, 20)])), [(0, 20)]);
}

#[test]
fn a_containing_span_replaces_what_it_covers() {
    assert_eq!(bounds(&inserted([(5, 10), (0, 20)])), [(0, 20)]);
}

#[test]
fn a_long_span_swallows_every_span_it_reaches() {
    let uptime = inserted([(0, 5), (10, 15), (20, 25), (30, 35), (2, 32)]);

    assert_eq!(bounds(&uptime), [(0, 35)]);
    assert_eq!(uptime.len(), 1);
}

#[test]
fn a_span_bridging_two_others_by_touching_them_merges_all_three() {
    assert_eq!(bounds(&inserted([(0, 5), (10, 15), (5, 10)])), [(0, 15)]);
}

#[test]
fn a_span_landing_in_a_gap_stays_separate() {
    let uptime = inserted([(0, 5), (30, 40), (10, 20)]);

    assert_eq!(bounds(&uptime), [(0, 5), (10, 20), (30, 40)]);
}

#[test]
fn insertion_and_bulk_construction_agree() {
    let spans = [(30, 40), (0, 10), (5, 20), (35, 50), (-10, -5)];

    assert_eq!(inserted(spans), from_spans(spans));
}

#[test]
fn sequences_covering_the_same_instants_are_equal() {
    let piecemeal = from_spans([(0, 5), (5, 10), (2, 7)]);
    let whole = from_spans([(0, 10)]);

    assert_eq!(piecemeal, whole);
    assert_eq!(piecemeal.len(), 1);
}

#[test]
fn len_counts_spans_after_merging_not_inserts() {
    let uptime = inserted([(0, 10), (5, 20), (15, 30)]);

    assert_eq!(uptime.len(), 1);
}

#[test]
fn negative_bounds_are_ordered_and_merged() {
    assert_eq!(
        bounds(&inserted([(-5, 5), (-20, -10), (-12, -3)])),
        [(-20, 5)]
    );
}

#[test]
fn the_timestamp_extremes_merge_without_overflowing() {
    // Merging only compares and takes min/max, so the widest possible spans
    // are no different from any others.
    let uptime = inserted([(i64::MIN, 0), (-1, i64::MAX)]);

    assert_eq!(bounds(&uptime), [(i64::MIN, i64::MAX)]);
}

#[test]
fn remove_takes_the_span_at_that_position() {
    let mut uptime = from_spans([(0, 10), (30, 40), (50, 60)]);

    assert_eq!(uptime.remove(1).bounds(), (30, 40));
    assert_eq!(bounds(&uptime), [(0, 10), (50, 60)]);
    assert_normalized(&uptime);
}

#[test]
#[should_panic(expected = "removal index")]
fn removing_past_the_end_panics() {
    let mut uptime = from_spans([(0, 10)]);

    let _ = uptime.remove(5);
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn indexing_past_the_end_panics() {
    let uptime = from_spans([(0, 10)]);

    let _ = uptime[5];
}

#[test]
fn a_span_covers_its_start_but_not_its_end() {
    let uptime = from_spans([(10, 20)]);

    assert!(uptime.contains(10));
    assert!(uptime.contains(19));
    assert!(!uptime.contains(20));
    assert_eq!(uptime.at(10).map(TimeIntervalEvent::bounds), Some((10, 20)));
    assert!(uptime.at(20).is_none());
}

#[test]
fn lookups_find_nothing_in_a_gap_or_past_the_edges() {
    let uptime = from_spans([(10, 20), (30, 40)]);

    assert!(!uptime.contains(25));
    assert!(!uptime.contains(5));
    assert!(!uptime.contains(50));
    assert!(uptime.at(25).is_none());
}

#[test]
fn lookups_pick_the_right_span_among_several() {
    let uptime = from_spans([(0, 10), (30, 40), (50, 60)]);

    assert_eq!(uptime.at(35).map(TimeIntervalEvent::bounds), Some((30, 40)));
    assert_eq!(uptime.at(55).map(TimeIntervalEvent::bounds), Some((50, 60)));
    assert_eq!(uptime.at(5).map(TimeIntervalEvent::bounds), Some((0, 10)));
}

#[test]
fn first_and_last_bracket_the_sequence() {
    let uptime = from_spans([(30, 40), (0, 10)]);

    assert_eq!(uptime.first().map(TimeIntervalEvent::bounds), Some((0, 10)));
    assert_eq!(uptime.last().map(TimeIntervalEvent::bounds), Some((30, 40)));
}

#[test]
fn nth_reads_by_position() {
    let uptime = from_spans([(30, 40), (0, 10)]);

    assert_eq!(uptime.nth(0).map(TimeIntervalEvent::bounds), Some((0, 10)));
    assert_eq!(uptime.nth(1).map(TimeIntervalEvent::bounds), Some((30, 40)));
    assert!(uptime.nth(2).is_none());
    assert_eq!(uptime[1].bounds(), (30, 40));
}

#[test]
fn an_empty_sequence_answers_nothing() {
    let uptime = TimeIntervalSequence::new("uptime");

    assert!(uptime.is_empty());
    assert_eq!(uptime.len(), 0);
    assert!(uptime.as_slice().is_empty());
    assert!(uptime.nth(0).is_none());
    assert!(uptime.first().is_none());
    assert!(uptime.last().is_none());
    assert!(uptime.at(0).is_none());
    assert!(!uptime.contains(0));
    assert_eq!(uptime.iter().count(), 0);
}

#[test]
fn a_new_timeline_says_what_it_describes() {
    assert_eq!(TimeIntervalSequence::new("uptime").label(), "uptime");
}

#[test]
fn attribute_reads_back_the_state_a_timeline_was_named_for() {
    let up = TimeIntervalSequence::from_spans("up", vec![span(0, 10)]);

    assert_eq!(up.attribute(), &Attribute::name("up"));
}

#[test]
fn transformation_starts_at_zero_and_records_what_transform_applied() {
    let alerts = TimeIntervalSequence::from_spans("alerts", vec![span(0, 10)]);

    assert_eq!(alerts.transformation(), (0, 0));
    assert_eq!(transformed(&alerts, -2, 2).transformation(), (-2, 2));
}

#[test]
fn a_transformed_timeline_says_so_in_its_label() {
    let alerts = TimeIntervalSequence::from_spans("alerts", vec![span(0, 10)]);

    assert_eq!(alerts.to_string(), "alerts");
    assert_eq!(transformed(&alerts, -2, 2).to_string(), "alerts[-2, 2]");
}

#[test]
fn timelines_of_different_states_are_never_equal() {
    let up = TimeIntervalSequence::from_spans("up", vec![span(0, 10)]);
    let reachable = TimeIntervalSequence::from_spans("reachable", vec![span(0, 10)]);

    // The same instants, described two ways — which is coverage equality,
    // and the reason the helpers above all build the one state.
    assert!(up.covers_same(&reachable));
    assert_ne!(up, reachable);
}

#[test]
fn from_spans_accepts_nothing() {
    let uptime = TimeIntervalSequence::from_spans("uptime", vec![]);

    assert!(uptime.is_empty());
    assert_eq!(uptime, TimeIntervalSequence::new("uptime"));
}

#[test]
fn clear_empties_the_sequence() {
    let mut uptime = from_spans([(0, 10), (30, 40)]);
    uptime.clear();

    assert!(uptime.is_empty());
}

#[test]
fn collecting_round_trips_through_into_iter() {
    let uptime = from_spans([(30, 40), (0, 10), (5, 20)]);

    let spans: Vec<TimeIntervalEvent<()>> = uptime.clone().into_iter().collect();
    assert_eq!(spans.len(), 2);

    let rebuilt = TimeIntervalSequence::from_spans(ATTRIBUTE, spans);
    assert_eq!(rebuilt, uptime);
}

#[test]
fn into_spans_round_trips_from_spans() {
    let uptime = from_spans([(0, 10), (5, 20), (30, 40)]);
    let spans = uptime.clone().into_spans();

    assert_eq!(spans.len(), 2);
    assert_eq!(TimeIntervalSequence::from_spans(ATTRIBUTE, spans), uptime);
}

#[test]
fn extend_normalizes_against_what_is_already_there() {
    let mut uptime = inserted([(0, 10)]);
    uptime.extend([span(5, 20), span(40, 50)]);
    assert_normalized(&uptime);

    assert_eq!(bounds(&uptime), [(0, 20), (40, 50)]);
}

#[test]
fn extend_onto_an_empty_sequence_still_normalizes() {
    let mut uptime = TimeIntervalSequence::new("uptime");
    uptime.extend([span(5, 20), span(0, 10)]);
    assert_normalized(&uptime);

    assert_eq!(bounds(&uptime), [(0, 20)]);
}

#[test]
fn every_operation_means_what_it_says_on_varied_shapes() {
    // Overlapping, nested, touching, disjoint, and interleaved operands,
    // each checked instant by instant against its boolean rule.
    let shapes: [OperandShapes<'_>; 8] = [
        (&[(0, 10)], &[(5, 15)]),
        (&[(0, 100)], &[(10, 20), (30, 40)]),
        (&[(0, 5)], &[(5, 9)]),
        (&[(0, 5)], &[(20, 30)]),
        (&[(0, 10), (20, 30)], &[(5, 25)]),
        (&[(0, 20)], &[(5, 10)]),
        (&[(0, 10), (20, 30), (40, 50)], &[(5, 15), (25, 45)]),
        (&[(10, 20)], &[(10, 20)]),
    ];

    for (left, right) in shapes {
        let a = from_spans(left.iter().copied());
        let b = from_spans(right.iter().copied());

        assert_all_operations(&a, &b);
        assert_all_operations(&b, &a);
    }
}

#[test]
fn every_operation_handles_an_empty_operand_on_either_side() {
    let a = from_spans([(0, 10), (20, 30)]);
    let empty = TimeIntervalSequence::new("empty");

    assert_all_operations(&a, &empty);
    assert_all_operations(&empty, &a);
    assert_all_operations(&empty, &empty);
}

#[test]
fn touching_sequences_unite_but_never_intersect() {
    let a = from_spans([(0, 5)]);
    let b = from_spans([(5, 9)]);

    assert_eq!(bounds(&a.union(&b)), [(0, 9)]);
    assert!(a.intersection(&b).is_empty());
    assert_eq!(bounds(&a.difference(&b)), [(0, 5)]);
    assert_eq!(bounds(&a.symmetric_difference(&b)), [(0, 9)]);
}

#[test]
fn one_long_span_against_many_short_ones() {
    let up = from_spans([(0, 100)]);
    let busy = from_spans([(10, 20), (30, 40)]);

    assert_same_coverage(&up.intersection(&busy), &busy);
    assert_eq!(
        bounds(&up.difference(&busy)),
        [(0, 10), (20, 30), (40, 100)]
    );
    assert_same_coverage(&up.union(&busy), &up);
    assert_eq!(
        bounds(&up.symmetric_difference(&busy)),
        [(0, 10), (20, 30), (40, 100)]
    );
}

#[test]
fn a_span_straddling_two_of_ours_carves_both() {
    // `theirs` reaches past the end of the first span into the second, so
    // `difference` must advance its own index without dropping `theirs`.
    let mine = from_spans([(0, 10), (20, 30)]);
    let theirs = from_spans([(5, 25)]);

    assert_eq!(bounds(&mine.difference(&theirs)), [(0, 5), (25, 30)]);
    assert_all_operations(&mine, &theirs);
}

#[test]
fn operations_leave_both_operands_untouched() {
    let a = from_spans([(0, 10), (20, 30)]);
    let b = from_spans([(5, 25)]);
    let (a_before, b_before) = (a.clone(), b.clone());

    let _ = a.union(&b);
    let _ = a.intersection(&b);
    let _ = a.difference(&b);
    let _ = a.symmetric_difference(&b);

    assert_eq!(a, a_before);
    assert_eq!(b, b_before);

    // And both are still usable for further work.
    assert_eq!(a.union(&b), a_before.union(&b_before));
}

#[test]
fn operations_are_commutative_where_they_should_be() {
    let a = from_spans([(0, 10), (20, 30)]);
    let b = from_spans([(5, 25), (40, 50)]);

    assert_eq!(a.union(&b), b.union(&a));
    assert_eq!(a.intersection(&b), b.intersection(&a));
    assert_eq!(a.symmetric_difference(&b), b.symmetric_difference(&a));

    // Difference is the one that is not.
    assert_ne!(a.difference(&b), b.difference(&a));
}

#[test]
fn operations_against_itself_collapse() {
    let a = from_spans([(0, 10), (20, 30)]);

    assert_same_coverage(&a.union(&a), &a);
    assert_same_coverage(&a.intersection(&a), &a);
    assert!(a.difference(&a).is_empty());
    assert!(a.symmetric_difference(&a).is_empty());
}

#[test]
fn the_empty_sequence_is_the_identity_it_should_be() {
    let a = from_spans([(0, 10), (20, 30)]);
    let empty = TimeIntervalSequence::new("empty");

    assert_same_coverage(&a.union(&empty), &a);
    assert!(a.intersection(&empty).is_empty());
    assert_same_coverage(&a.difference(&empty), &a);
    assert!(empty.difference(&a).is_empty());
    assert_same_coverage(&a.symmetric_difference(&empty), &a);
}

#[test]
fn absorption_laws_hold() {
    let a = from_spans([(0, 10), (20, 30)]);
    let b = from_spans([(5, 25)]);

    assert_same_coverage(&a.union(&a.intersection(&b)), &a);
    assert_same_coverage(&a.intersection(&a.union(&b)), &a);
}

#[test]
fn intersection_distributes_over_union_and_the_dual() {
    let a = from_spans([(0, 20), (40, 60)]);
    let b = from_spans([(10, 30)]);
    let c = from_spans([(15, 50)]);

    assert_same_coverage(
        &a.intersection(&b.union(&c)),
        &a.intersection(&b).union(&a.intersection(&c)),
    );
    assert_same_coverage(
        &a.union(&b.intersection(&c)),
        &a.union(&b).intersection(&a.union(&c)),
    );
}

#[test]
fn de_morgan_holds_relative_to_the_left_operand() {
    let a = from_spans([(0, 60)]);
    let b = from_spans([(10, 30)]);
    let c = from_spans([(20, 50)]);

    assert_same_coverage(
        &a.difference(&b.union(&c)),
        &a.difference(&b).intersection(&a.difference(&c)),
    );
    assert_same_coverage(
        &a.difference(&b.intersection(&c)),
        &a.difference(&b).union(&a.difference(&c)),
    );
}

#[test]
fn the_operations_agree_with_each_other() {
    let a = from_spans([(0, 10), (20, 30), (40, 50)]);
    let b = from_spans([(5, 25), (45, 60)]);

    // Symmetric difference is everything covered, minus what both share.
    assert_same_coverage(
        &a.symmetric_difference(&b),
        &a.union(&b).difference(&a.intersection(&b)),
    );
    // What is only ours, plus what we share, is everything of ours.
    assert_same_coverage(&a.difference(&b).union(&a.intersection(&b)), &a);
}

#[test]
fn operations_handle_negative_bounds() {
    let a = from_spans([(-20, -10), (-5, 5)]);
    let b = from_spans([(-15, 0)]);

    assert_eq!(bounds(&a.union(&b)), [(-20, 5)]);
    assert_eq!(bounds(&a.intersection(&b)), [(-15, -10), (-5, 0)]);
    assert_eq!(bounds(&a.difference(&b)), [(-20, -15), (0, 5)]);
}

#[test]
fn operations_survive_the_timestamp_extremes() {
    let widest = from_spans([(i64::MIN, i64::MAX)]);
    let middle = from_spans([(-1, 1)]);

    assert_same_coverage(&widest.union(&middle), &widest);
    assert_same_coverage(&widest.intersection(&middle), &middle);
    assert_eq!(
        bounds(&widest.difference(&middle)),
        [(i64::MIN, -1), (1, i64::MAX)]
    );
}

#[test]
fn borrowed_iteration_leaves_the_sequence_usable() {
    let uptime = from_spans([(0, 10), (30, 40)]);

    let seen: Vec<(i64, i64)> = (&uptime)
        .into_iter()
        .map(TimeIntervalEvent::bounds)
        .collect();

    assert_eq!(seen, [(0, 10), (30, 40)]);
    assert_eq!(uptime.len(), 2);
}

#[test]
fn an_empty_sequence_covers_no_time() {
    assert_eq!(TimeIntervalSequence::new("uptime").active_duration(), 0);
    assert_eq!(from_spans([]).active_duration(), 0);
}

#[test]
fn duration_totals_the_gaps_between_spans_away() {
    assert_eq!(from_spans([(0, 10)]).active_duration(), 10);
    assert_eq!(from_spans([(0, 10), (30, 40)]).active_duration(), 20);
    assert_eq!(from_spans([(-10, -4)]).active_duration(), 6);
}

#[test]
fn duration_counts_covered_instants_not_inserted_spans() {
    // Three spans covering `[0, 20)` between them, however they arrived.
    let overlapping = from_spans([(0, 10), (5, 15), (12, 20)]);
    assert_eq!(overlapping.len(), 1);
    assert_eq!(overlapping.active_duration(), 20);

    // Touching spans merge, and the whole is the sum of its parts here.
    assert_eq!(from_spans([(0, 5), (5, 9)]).active_duration(), 9);

    // The same coverage built any other way agrees.
    assert_eq!(inserted([(12, 20), (0, 10), (5, 15)]).active_duration(), 20);
    assert_eq!(from_spans([(0, 20)]).active_duration(), 20);
}

#[test]
fn inserting_adds_only_what_was_not_covered() {
    let mut uptime = inserted([(0, 10)]);
    assert_eq!(uptime.active_duration(), 10);

    // Into a gap: all of it is new.
    uptime.insert(span(30, 40));
    assert_eq!(uptime.active_duration(), 20);

    // Overlapping: only the part past the existing end.
    uptime.insert(span(5, 15));
    assert_eq!(uptime.active_duration(), 25);

    // Wholly covered: nothing new at all.
    uptime.insert(span(2, 8));
    assert_eq!(uptime.active_duration(), 25);

    // Bridging the gap adds the gap itself, and nothing more.
    uptime.insert(span(15, 30));
    assert_eq!(uptime.len(), 1);
    assert_eq!(uptime.active_duration(), 40);
    assert_normalized(&uptime);
}

#[test]
fn a_span_swallowing_several_trades_them_for_the_whole() {
    let mut uptime = inserted([(0, 10), (20, 30), (40, 50)]);
    assert_eq!(uptime.active_duration(), 30);

    uptime.insert(span(5, 45));

    assert_eq!(bounds(&uptime), [(0, 50)]);
    assert_eq!(uptime.active_duration(), 50);
}

#[test]
fn removing_gives_back_exactly_what_that_span_covered() {
    let mut uptime = from_spans([(0, 10), (30, 40), (50, 55)]);
    assert_eq!(uptime.active_duration(), 25);

    uptime.remove(1);
    assert_eq!(uptime.active_duration(), 15);
    assert_normalized(&uptime);

    uptime.remove(0);
    assert_eq!(uptime.active_duration(), 5);

    uptime.remove(0);
    assert_eq!(uptime.active_duration(), 0);
    assert!(uptime.is_empty());
}

#[test]
fn clearing_resets_the_duration() {
    let mut uptime = from_spans([(0, 10), (30, 40)]);
    uptime.clear();

    assert_eq!(uptime.active_duration(), 0);
}

#[test]
fn extending_totals_the_same_as_building_at_once() {
    let mut piecemeal = from_spans([(0, 10)]);
    piecemeal.extend([span(5, 20), span(40, 50)]);

    assert_eq!(piecemeal.active_duration(), 30);
    assert_eq!(
        piecemeal.active_duration(),
        from_spans([(0, 20), (40, 50)]).active_duration(),
    );
}

#[test]
fn the_operations_account_for_every_instant_they_move() {
    let a = from_spans([(0, 10), (20, 30)]);
    let b = from_spans([(5, 25)]);

    // The pieces of `a` partition `a`: what it shares with `b` plus what it
    // keeps to itself is everything it covered, with nothing double-counted.
    assert_eq!(
        a.intersection(&b).active_duration() + a.difference(&b).active_duration(),
        a.active_duration(),
    );

    // Inclusion-exclusion, the same statement from the union's side.
    assert_eq!(
        a.union(&b).active_duration() + a.intersection(&b).active_duration(),
        a.active_duration() + b.active_duration(),
    );

    // XOR is the union minus the shared part, counted once.
    assert_eq!(
        a.symmetric_difference(&b).active_duration(),
        a.union(&b).active_duration() - a.intersection(&b).active_duration(),
    );
}

#[test]
fn duration_is_exact_where_a_single_span_would_saturate() {
    // `TimeIntervalEvent::duration` clamps this to `i64::MAX`; the sequence
    // must not, and `u64` is exactly wide enough to say so.
    let widest = from_spans([(i64::MIN, i64::MAX)]);
    assert_eq!(widest.active_duration(), u64::MAX);
    assert_eq!(widest.as_slice()[0].duration(), i64::MAX);

    // Carving the middle out leaves everything but those two ticks.
    let carved = widest.difference(&from_spans([(-1, 1)]));
    assert_eq!(bounds(&carved), [(i64::MIN, -1), (1, i64::MAX)]);
    assert_eq!(carved.active_duration(), u64::MAX - 2);

    // And putting it back restores the total, through the merge branch.
    let mut restored = carved;
    restored.insert(span(-1, 1));
    assert_same_coverage(&restored, &widest);
    assert_eq!(restored.active_duration(), u64::MAX);
}

#[test]
fn transforming_by_nothing_changes_nothing() {
    let uptime = from_spans([(0, 10), (20, 30)]);

    assert_eq!(transformed(&uptime, 0, 0), uptime);
    assert!(transformed(&TimeIntervalSequence::new(ATTRIBUTE), 3, -3).is_empty());
}

#[test]
fn an_equal_shift_translates_every_instant() {
    let uptime = from_spans([(0, 10), (20, 30)]);
    let later = transformed(&uptime, 7, 7);

    assert_eq!(bounds(&later), [(7, 17), (27, 37)]);

    // Nothing widened or narrowed, so the covered instants only moved.
    assert_eq!(later.active_duration(), uptime.active_duration());
    for timestamp in -5..=105 {
        assert_eq!(later.contains(timestamp + 7), uptime.contains(timestamp));
    }
}

#[test]
fn widening_merges_a_gap_it_closes_and_leaves_a_narrower_one_open() {
    // Two ticks of slack on each side close a four-tick gap to exactly
    // zero, and touching spans cover an unbroken stretch.
    assert_eq!(
        bounds(&transformed(&from_spans([(0, 10), (14, 20)]), -2, 2)),
        [(-2, 22)]
    );

    // One tick more of gap than the shift can close keeps them apart.
    assert_eq!(
        bounds(&transformed(&from_spans([(0, 10), (15, 20)]), -2, 2)),
        [(-2, 12), (13, 22)]
    );
}

#[test]
fn narrowing_drops_spans_no_wider_than_the_shrinkage() {
    // Narrowing by three: a span of exactly three ticks has no interior
    // left, while one of four keeps a single tick.
    let uptime = from_spans([(0, 3), (10, 14), (20, 40)]);

    assert_eq!(bounds(&transformed(&uptime, 3, 0)), [(13, 14), (23, 40)]);
}

#[test]
fn a_long_enough_widening_collapses_a_whole_chain() {
    let flapping = from_spans([(0, 1), (5, 6), (10, 11), (15, 16)]);

    assert_eq!(bounds(&transformed(&flapping, -3, 3)), [(-3, 19)]);
}

#[test]
fn widening_only_merges_and_narrowing_only_drops() {
    let uptime = from_spans([(0, 10), (12, 22), (24, 25)]);

    // Widening keeps every instant it started from — it can merge spans,
    // but never removes one.
    let wider = transformed(&uptime, -2, 0);
    assert!(wider.len() <= uptime.len());
    for timestamp in -5..=105 {
        assert!(!uptime.contains(timestamp) || wider.contains(timestamp));
    }

    // Narrowing covers nothing new — it can drop spans, but never brings
    // two together, so every gap it started with is still there.
    let narrower = transformed(&uptime, 2, 0);
    assert!(narrower.len() <= uptime.len());
    for timestamp in -5..=105 {
        assert!(!narrower.contains(timestamp) || uptime.contains(timestamp));
    }
    assert_eq!(bounds(&narrower), [(2, 10), (14, 22)]);
}

#[test]
fn transforms_add_up_until_one_of_them_loses_something() {
    let uptime = from_spans([(0, 10), (20, 30)]);

    // Nothing merges or vanishes along the way, so the shifts just sum.
    let stepwise = transformed(&transformed(&uptime, 1, 2), 3, -1);
    assert_eq!(stepwise, transformed(&uptime, 4, 1));

    // Once a widening has swallowed a gap, narrowing back cannot reopen it:
    // the two spans are one span now, and it simply narrows.
    let swallowed = transformed(&from_spans([(0, 10), (12, 22)]), -1, 1);
    assert_eq!(bounds(&swallowed), [(-1, 23)]);
    assert_eq!(bounds(&transformed(&swallowed, 1, -1)), [(0, 22)]);
}

#[test]
fn a_shift_past_the_end_of_time_is_rejected() {
    let late = from_spans([(0, i64::MAX - 1)]);

    assert_eq!(
        late.transform(0, 2),
        Err(IntervalError::BoundOverflow {
            bound: i64::MAX - 1,
            shift: 2,
        })
    );

    let early = from_spans([(i64::MIN + 1, 0)]);
    assert_eq!(
        early.transform(-2, 0),
        Err(IntervalError::BoundOverflow {
            bound: i64::MIN + 1,
            shift: -2,
        })
    );

    // The receiver is borrowed, so a rejected transform costs it nothing.
    assert_eq!(bounds(&late), [(0, i64::MAX - 1)]);
    assert_eq!(bounds(&early), [(i64::MIN + 1, 0)]);
}

#[test]
fn a_bound_landing_exactly_on_the_limit_is_accepted() {
    assert_eq!(
        bounds(&transformed(&from_spans([(0, i64::MAX - 1)]), 0, 1)),
        [(0, i64::MAX)]
    );
    assert_eq!(
        bounds(&transformed(&from_spans([(i64::MIN + 1, 0)]), -1, 0)),
        [(i64::MIN, 0)]
    );
}

#[test]
fn equality_ignores_the_duration_and_stays_coverage_equality() {
    let piecemeal = from_spans([(0, 5), (5, 10), (2, 7)]);
    let whole = from_spans([(0, 10)]);

    assert_eq!(piecemeal, whole);
    assert_eq!(piecemeal.active_duration(), whole.active_duration());

    // Same span count and same total, but not the same instants.
    let shifted = from_spans([(0, 5), (10, 15)]);
    let elsewhere = from_spans([(20, 25), (30, 35)]);
    assert_eq!(shifted.active_duration(), elsewhere.active_duration());
    assert_ne!(shifted, elsewhere);
}
