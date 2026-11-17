//! Properties of timelines checked against the instant-set oracle, over many
//! seeded random inputs. A failure names its seed: replay it by running the
//! property with `for_each_seed` narrowed to that one seed.

mod common;

use chronoloom::{TimeIntervalSequence, TimePointSequence};

use common::{
    bounds, for_each_seed, instants, point_events, spans, InstantSet, Random, DOMAIN_END,
};

/// Seeds per property.
const CASES: u64 = 500;

/// Most spans an operand is built from.
const SPAN_COUNT: u64 = 12;

/// A random timeline and the oracle of the instants it covers.
fn timeline(random: &mut Random, attribute: &str) -> (TimeIntervalSequence, InstantSet) {
    let drawn = spans(random, SPAN_COUNT);
    let oracle = InstantSet::from_spans(&drawn);
    (TimeIntervalSequence::from_spans(attribute, drawn), oracle)
}

#[test]
fn union_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (a, a_instants) = timeline(random, "a");
        let (b, b_instants) = timeline(random, "b");
        assert_eq!(
            bounds(&a.union(&b)),
            a_instants.union(&b_instants).to_spans()
        );
    });
}

#[test]
fn intersection_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (a, a_instants) = timeline(random, "a");
        let (b, b_instants) = timeline(random, "b");
        assert_eq!(
            bounds(&a.intersection(&b)),
            a_instants.intersection(&b_instants).to_spans()
        );
    });
}

#[test]
fn difference_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (a, a_instants) = timeline(random, "a");
        let (b, b_instants) = timeline(random, "b");
        assert_eq!(
            bounds(&a.difference(&b)),
            a_instants.difference(&b_instants).to_spans()
        );
    });
}

#[test]
fn symmetric_difference_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (a, a_instants) = timeline(random, "a");
        let (b, b_instants) = timeline(random, "b");
        assert_eq!(
            bounds(&a.symmetric_difference(&b)),
            a_instants.symmetric_difference(&b_instants).to_spans()
        );
    });
}

#[test]
fn transform_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (timeline, oracle) = timeline(random, "a");
        let alpha = random.between(-8, 9);
        let beta = random.between(-8, 9);
        let transformed = timeline
            .transform(alpha, beta)
            .expect("shifts this small never overflow");
        assert_eq!(
            bounds(&transformed),
            oracle.transform(alpha, beta).to_spans(),
            "alpha {alpha}, beta {beta}"
        );
    });
}

#[test]
fn from_spans_is_normalized_and_measured() {
    for_each_seed(CASES, |random, _| {
        let (timeline, oracle) = timeline(random, "a");
        let spans = bounds(&timeline);

        // Sorted, disjoint and never touching: each span ends strictly before
        // the next starts.
        for pair in spans.windows(2) {
            assert!(
                pair[0].1 < pair[1].0,
                "{:?} and {:?} should have merged",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(timeline.active_duration(), oracle.len());
    });
}

#[test]
fn contains_matches_the_oracle() {
    for_each_seed(CASES, |random, _| {
        let (timeline, oracle) = timeline(random, "a");
        for _ in 0..16 {
            let instant = instants(random);
            assert_eq!(
                timeline.contains(instant),
                oracle.contains(instant),
                "instant {instant}"
            );
        }
    });
}

#[test]
fn building_twice_gives_identical_output() {
    for_each_seed(CASES, |random, _| {
        let drawn = spans(random, SPAN_COUNT);
        let first = TimeIntervalSequence::from_spans("a", drawn.clone());
        let second = TimeIntervalSequence::from_spans("a", drawn);
        assert_eq!(first.as_slice(), second.as_slice());

        let events = point_events(random, SPAN_COUNT);
        let first = TimePointSequence::from_events("p", events.clone());
        let second = TimePointSequence::from_events("p", events);
        assert_eq!(first.as_slice(), second.as_slice());
    });
}

#[test]
fn generated_inputs_stay_in_the_domain() {
    for_each_seed(CASES, |random, _| {
        for span in spans(random, SPAN_COUNT) {
            assert!(span.start() >= 0 && span.end() <= DOMAIN_END);
        }
        for event in point_events(random, SPAN_COUNT) {
            assert!((0..DOMAIN_END).contains(&event.timestamp()));
        }
    });
}
