//! Properties of timelines checked against the instant-set oracle, over many
//! seeded random inputs. A failure names its seed: replay it by running the
//! property with `for_each_seed` narrowed to that one seed.

mod common;

use std::collections::BTreeSet;

use chronoloom::{
    Condition, Extent, Formula, OpenEnd, Pattern, Processor, StreamSystem, TimeIntervalSequence,
    TimePointEvent, TimePointSequence,
};

use common::{
    assert_knows, bounds, extent, for_each_seed, instants, observed_instants, point_events,
    property_cases, span, spans, InstantSet, Knowledge, Random, Truth, DOMAIN_END, DOMAIN_START,
};

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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
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
    for_each_seed(property_cases(), |random, _| {
        for span in spans(random, SPAN_COUNT) {
            assert!(span.start() >= 0 && span.end() <= DOMAIN_END);
        }
        for event in point_events(random, SPAN_COUNT) {
            assert!((0..DOMAIN_END).contains(&event.timestamp()));
        }
    });
}

/// A random timeline observed over a random extent, with what the oracle knows
/// of it.
fn observed_timeline(random: &mut Random, attribute: &str) -> (TimeIntervalSequence, Knowledge) {
    let drawn = spans(random, SPAN_COUNT);
    let extent = extent(random);
    let knowledge = Knowledge::new(observed_instants(&extent), &InstantSet::from_spans(&drawn));
    let timeline = TimeIntervalSequence::from_spans(attribute, drawn).with_observed(extent);
    (timeline, knowledge)
}

#[test]
fn observed_timelines_match_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (timeline, knowledge) = observed_timeline(random, "a");
        assert_knows(&timeline, &knowledge, "construction");
    });
}

#[test]
fn three_valued_union_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (a, known_a) = observed_timeline(random, "a");
        let (b, known_b) = observed_timeline(random, "b");
        assert_knows(&a.union(&b), &known_a.union(&known_b), "union");
    });
}

#[test]
fn three_valued_intersection_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (a, known_a) = observed_timeline(random, "a");
        let (b, known_b) = observed_timeline(random, "b");
        assert_knows(
            &a.intersection(&b),
            &known_a.intersection(&known_b),
            "intersection",
        );
    });
}

#[test]
fn three_valued_difference_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (a, known_a) = observed_timeline(random, "a");
        let (b, known_b) = observed_timeline(random, "b");
        assert_knows(
            &a.difference(&b),
            &known_a.difference(&known_b),
            "difference",
        );
    });
}

#[test]
fn three_valued_symmetric_difference_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (a, known_a) = observed_timeline(random, "a");
        let (b, known_b) = observed_timeline(random, "b");
        assert_knows(
            &a.symmetric_difference(&b),
            &known_a.symmetric_difference(&known_b),
            "symmetric difference",
        );
    });
}

#[test]
fn three_valued_complement_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (timeline, knowledge) = observed_timeline(random, "a");
        assert_knows(
            &timeline.complement(),
            &knowledge.complement(),
            "complement",
        );
    });
}

#[test]
fn three_valued_transform_matches_the_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let (timeline, knowledge) = observed_timeline(random, "a");
        let alpha = random.between(-8, 9);
        let beta = random.between(-8, 9);
        let transformed = timeline.transform(alpha, beta).expect("small shifts");
        assert_knows(
            &transformed,
            &knowledge.transform(alpha, beta),
            &format!("transform({alpha}, {beta})"),
        );
    });
}

#[test]
fn complementing_twice_covers_the_same_within_the_extent() {
    for_each_seed(property_cases(), |random, _| {
        let (timeline, _) = observed_timeline(random, "a");
        let twice = timeline.complement().complement();
        assert!(twice.covers_same(&timeline));
        assert_eq!(twice.observed(), timeline.observed());
    });
}

/// The oracle's window rule for the transform, checked against its meaning: a
/// result instant is known when every way of filling in the unknown instants
/// gives it the same state.
#[test]
fn transform_knows_what_every_completion_agrees_on() {
    for_each_seed(property_cases(), |random, _| {
        // A few unknown instants inside [0, 10); everything else is observed.
        let holes: Vec<i64> = (0..10).filter(|_| random.chance(1, 3)).take(6).collect();
        let mut observed = vec![span(i64::MIN, 0), span(10, i64::MAX)];
        let mut active = Vec::new();
        for instant in (0..10).filter(|instant| !holes.contains(instant)) {
            observed.push(span(instant, instant + 1));
            if random.chance(1, 2) {
                active.push(span(instant, instant + 1));
            }
        }
        let timeline = TimeIntervalSequence::from_spans("a", active.clone())
            .with_observed(Extent::from_spans(observed));

        let alpha = random.between(-4, 5);
        let beta = random.between(-4, 5);
        let transformed = timeline.transform(alpha, beta).expect("small shifts");

        let mut ever_active = BTreeSet::new();
        let mut ever_inactive = BTreeSet::new();
        for fill in 0..(1_u32 << holes.len()) {
            let mut completed = active.clone();
            for (bit, &hole) in holes.iter().enumerate() {
                if (fill >> bit) & 1 == 1 {
                    completed.push(span(hole, hole + 1));
                }
            }
            let result = InstantSet::from_spans(&completed).transform(alpha, beta);
            for instant in -12..22 {
                if result.contains(instant) {
                    ever_active.insert(instant);
                } else {
                    ever_inactive.insert(instant);
                }
            }
        }

        for instant in -12..22 {
            let expected = match (
                ever_active.contains(&instant),
                ever_inactive.contains(&instant),
            ) {
                (true, false) => Truth::Active,
                (false, true) => Truth::Inactive,
                _ => Truth::Unknown,
            };
            let found = if transformed.contains(instant) {
                Truth::Active
            } else if transformed.is_observed(instant) {
                Truth::Inactive
            } else {
                Truth::Unknown
            };
            assert_eq!(
                found, expected,
                "instant {instant}, transform({alpha}, {beta}), holes {holes:?}"
            );
        }
    });
}

/// First instant a generated slice range can start at.
const RANGE_START: i64 = -4;

/// A random range of time around the domain, as its two bounds.
fn range(random: &mut Random) -> (i64, i64) {
    let start = random.between(RANGE_START, DOMAIN_END - 1);
    (start, start + random.between(1, DOMAIN_END - start + 1))
}

#[test]
fn slicing_a_timeline_intersects_spans_and_extent_with_the_range() {
    for_each_seed(property_cases(), |random, _| {
        let (start, end) = range(random);
        let (timeline, _) = timeline(random, "a");
        let timeline = timeline.with_observed(extent(random));
        let window = InstantSet::from_spans(&[span(start, end)]);

        let sliced = timeline.slice(start..end);

        let covered = InstantSet::from_spans(timeline.iter());
        assert_eq!(bounds(&sliced), covered.intersection(&window).to_spans());
        let observed = observed_instants(timeline.observed()).intersection(&window);
        assert_eq!(observed_instants(sliced.observed()), observed);
        assert_eq!(sliced.label(), timeline.label());
    });
}

#[test]
fn slicing_points_keeps_the_events_and_extent_inside_the_range() {
    for_each_seed(property_cases(), |random, _| {
        let (start, end) = range(random);
        let drawn = point_events(random, SPAN_COUNT);
        let readings =
            TimePointSequence::from_events("readings", drawn).with_observed(extent(random));
        let window = InstantSet::from_spans(&[span(start, end)]);

        let sliced = readings.slice(start..end);

        let expected: Vec<_> = readings
            .iter()
            .filter(|event| (start..end).contains(&event.timestamp()))
            .cloned()
            .collect();
        assert_eq!(sliced.as_slice(), expected.as_slice());
        let observed = observed_instants(readings.observed()).intersection(&window);
        assert_eq!(observed_instants(sliced.observed()), observed);
        assert_eq!(sliced.label(), "readings");
    });
}

#[test]
fn slicing_twice_is_slicing_once_to_the_overlap() {
    for_each_seed(property_cases(), |random, _| {
        let (a, b) = range(random);
        let (c, d) = range(random);
        let (timeline, _) = timeline(random, "a");
        let timeline = timeline.with_observed(extent(random));
        assert_eq!(
            timeline.slice(a..b).slice(c..d),
            timeline.slice(a.max(c)..b.min(d))
        );

        let readings = TimePointSequence::from_events("r", point_events(random, SPAN_COUNT))
            .with_observed(extent(random));
        assert_eq!(
            readings.slice(a..b).slice(c..d),
            readings.slice(a.max(c)..b.min(d))
        );
    });
}

/// A random system for formulas to read: two point members of small numbers
/// and two timelines, each observed over a random extent, with what the oracle
/// knows of the timelines.
struct World {
    system: StreamSystem,
    points: [TimePointSequence<i64>; 2],
    states: [Knowledge; 2],
}

const POINT_KEYS: [&str; 2] = ["p", "q"];
const STATE_KEYS: [&str; 2] = ["s", "t"];

fn world(random: &mut Random) -> World {
    let mut system = StreamSystem::new();
    let points = POINT_KEYS.map(|key| {
        let drawn = random.below(SPAN_COUNT + 1);
        let events = (0..drawn)
            .map(|_| TimePointEvent::new(random.between(0, DOMAIN_END), random.between(0, 4)))
            .collect();
        let sequence = TimePointSequence::from_events(key, events).with_observed(extent(random));
        system.insert_stream(key, sequence.clone()).unwrap();
        sequence
    });
    let states = STATE_KEYS.map(|key| {
        let (timeline, knowledge) = observed_timeline(random, key);
        system.insert_stream(key, timeline).unwrap();
        knowledge
    });

    World {
        system,
        points,
        states,
    }
}

/// A random condition on the small numbers, with its meaning as a closure.
fn condition(random: &mut Random) -> (Condition<i64>, Box<dyn Fn(i64) -> bool>) {
    let bound = random.between(0, 4);
    match random.below(5) {
        0 => (Condition::Equals(bound), Box::new(move |v| v == bound)),
        1 => (Condition::Less(bound), Box::new(move |v| v < bound)),
        2 => (
            Condition::GreaterOrEqual(bound),
            Box::new(move |v| v >= bound),
        ),
        3 => (
            Condition::Not(Box::new(Condition::Equals(bound))),
            Box::new(move |v| v != bound),
        ),
        _ => (Condition::Any, Box::new(|_| true)),
    }
}

/// The first instant of the run of `observed` instants holding `instant`,
/// with nothing happening before the domain.
fn run_start(instant: i64, observed: &dyn Fn(i64) -> bool) -> i64 {
    let mut start = instant;
    while start > -1 && observed(start - 1) {
        start -= 1;
    }
    start
}

fn truth(known: Option<bool>) -> Truth {
    match known {
        Some(true) => Truth::Active,
        Some(false) => Truth::Inactive,
        None => Truth::Unknown,
    }
}

/// A random leaf and what the oracle knows of it, from the leaf's definition
/// at each instant.
fn leaf(random: &mut Random, world: &World) -> (Formula, Knowledge) {
    let first = random.below(2) as usize;
    let second = random.below(2) as usize;
    match random.below(5) {
        0 => (
            Formula::state(STATE_KEYS[first]),
            world.states[first].clone(),
        ),
        1 => {
            let points = &world.points[first];
            let (condition, test) = condition(random);
            let formula = Formula::occurs(Pattern::new(POINT_KEYS[first], condition));
            let knowledge = Knowledge::from_states(|instant| {
                truth(
                    points
                        .is_observed(instant)
                        .then(|| points.get(instant).iter().any(|event| test(*event.value()))),
                )
            });
            (formula, knowledge)
        }
        2 => {
            let points = &world.points[first];
            let (condition, test) = condition(random);
            let formula = Formula::while_matching(Pattern::new(POINT_KEYS[first], condition));
            let knowledge = Knowledge::from_states(|instant| {
                truth(points.is_observed(instant).then_some(()).and_then(|()| {
                    let from = run_start(instant, &|t| points.is_observed(t));
                    points
                        .iter()
                        .rfind(|event| (from..=instant).contains(&event.timestamp()))
                        .map(|event| test(*event.value()))
                }))
            });
            (formula, knowledge)
        }
        _ => {
            let (start_condition, starts) = condition(random);
            let (end_condition, ends) = condition(random);
            let head = random.chance(1, 2);
            let (opening, closing) = (&world.points[first], &world.points[second]);
            let same_member = first == second;
            let formula = Formula::between_with(
                Pattern::new(POINT_KEYS[first], start_condition),
                Pattern::new(POINT_KEYS[second], end_condition),
                if head { OpenEnd::Head } else { OpenEnd::Extent },
            );
            let latest = [opening, closing]
                .iter()
                .filter_map(|points| points.last().map(TimePointEvent::timestamp))
                .max();
            let head_of = latest.map_or(i64::MIN, |timestamp| timestamp + 1);
            let observed = |instant: i64| {
                opening.is_observed(instant)
                    && closing.is_observed(instant)
                    && (!head || instant < head_of)
            };
            let knowledge = Knowledge::from_states(|instant| {
                if !observed(instant) {
                    return Truth::Unknown;
                }
                let (mut open, mut opened_now) = (false, false);
                for at in run_start(instant, &observed)..=instant {
                    let reads: Vec<(bool, bool)> = if same_member {
                        opening
                            .get(at)
                            .iter()
                            .map(|event| (starts(*event.value()), ends(*event.value())))
                            .collect()
                    } else {
                        opening
                            .get(at)
                            .iter()
                            .map(|event| (starts(*event.value()), false))
                            .chain(
                                closing
                                    .get(at)
                                    .iter()
                                    .map(|event| (false, ends(*event.value()))),
                            )
                            .collect()
                    };
                    for (opens, closes) in reads {
                        if !open && opens {
                            open = true;
                            opened_now |= at == instant;
                        } else if open && closes {
                            open = false;
                        }
                    }
                }
                truth(Some(open || opened_now))
            });
            (formula, knowledge)
        }
    }
}

/// A random formula of at most `depth` levels above its leaves, and what the
/// oracle knows of it.
fn formula(random: &mut Random, world: &World, depth: u32) -> (Formula, Knowledge) {
    if depth == 0 || random.chance(1, 3) {
        return leaf(random, world);
    }

    match random.below(4) {
        0 => {
            let (inner, known) = formula(random, world, depth - 1);
            (!inner, known.complement())
        }
        1 => {
            let (alpha, beta) = (random.between(-3, 4), random.between(-3, 4));
            let (inner, known) = formula(random, world, depth - 1);
            (inner.shift(alpha, beta), known.transform(alpha, beta))
        }
        choice => {
            let count = random.between(2, 4);
            let parts: Vec<(Formula, Knowledge)> = (0..count)
                .map(|_| formula(random, world, depth - 1))
                .collect();
            let (formulas, mut known): (Vec<Formula>, Vec<Knowledge>) = parts.into_iter().unzip();
            let first = known.remove(0);
            if choice == 2 {
                let combined = known.iter().fold(first, |all, next| all.intersection(next));
                (Formula::And(formulas), combined)
            } else {
                let combined = known.iter().fold(first, |all, next| all.union(next));
                (Formula::Or(formulas), combined)
            }
        }
    }
}

/// Evaluate `formula`, which never fails over a world's members.
fn evaluate(formula: &Formula, world: &World) -> TimeIntervalSequence {
    let mut system = world.system.clone();
    system
        .add_processor(Processor::new("evaluated", formula.clone()))
        .expect("a random formula reads only members of its world");
    system
        .intervals("evaluated")
        .expect("a formula derives a timeline")
        .clone()
}

#[test]
fn random_formulas_match_the_three_valued_oracle() {
    for_each_seed(property_cases(), |random, _| {
        let world = world(random);
        let (formula, expected) = formula(random, &world, 3);
        let timeline = evaluate(&formula, &world);

        // Far enough inside the range the oracle knows that the windows of
        // nested shifts never reach past it.
        for instant in (DOMAIN_START - 8)..(DOMAIN_END + 8) {
            let state = expected.state(instant);
            assert_eq!(
                timeline.is_observed(instant),
                state != Truth::Unknown,
                "is_observed({instant}), {formula}"
            );
            assert_eq!(
                timeline.contains(instant),
                state == Truth::Active,
                "contains({instant}), {formula}"
            );
        }
        assert_eq!(timeline.label(), formula.to_string());
    });
}

/// Whether `formula` is an `And` of `Or`s of literals.
fn is_conjunctive_normal_form(formula: &Formula) -> bool {
    let atom = |formula: &Formula| match formula {
        Formula::State(_)
        | Formula::Between { .. }
        | Formula::While { .. }
        | Formula::Occurs(_) => true,
        Formula::Shift { formula, .. } => is_conjunctive_normal_form(formula),
        _ => false,
    };
    let literal = |formula: &Formula| match formula {
        Formula::Not(inner) => atom(inner),
        other => atom(other),
    };

    match formula {
        Formula::And(clauses) => clauses.iter().all(|clause| match clause {
            Formula::Or(literals) => literals.iter().all(literal),
            _ => false,
        }),
        _ => false,
    }
}

#[test]
fn the_conjunctive_normal_form_denotes_the_same_timeline() {
    for_each_seed(property_cases(), |random, _| {
        let world = world(random);
        let (formula, _) = formula(random, &world, 3);
        let normal = formula.conjunctive_normal_form();

        assert!(is_conjunctive_normal_form(&normal), "{normal}");
        assert_eq!(normal.conjunctive_normal_form(), normal, "{normal}");

        let original = evaluate(&formula, &world);
        let rewritten = evaluate(&normal, &world);
        assert!(rewritten.covers_same(&original), "{formula} vs {normal}");
        assert_eq!(rewritten.observed(), original.observed(), "{formula}");
    });
}
