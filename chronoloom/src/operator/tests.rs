//! The property of the online operators: random inputs cut at random
//! boundaries, stepped chunk by chunk, join into what `batch` gives over the
//! whole input.
//!
//! The operators are crate-private, so unlike every other test of the crate
//! this one lives beside the source, in a test-only module. The generators are
//! a small copy of those of `tests/common/random.rs`, which an integration
//! test reaches through the public API and this module cannot include.

use std::collections::BTreeMap;

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::processing::{
    render, Condition, Events, Formula, Group, Kind, OpenEnd, Pattern, ProcessError, Recipe,
};
use crate::sequence_system::{Key, Sequence, SequenceSystem};
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use crate::values::Value;
use crate::AnyPointSequence;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::online_points::OnlinePoints;
use super::online_recipe::OnlineRecipe;
use super::plan::PointsPlan;
use super::plan::{compile_recipe, RecipePlan};
use super::TimelinePlan;

/// First instant generated inputs use.
const DOMAIN_START: i64 = 0;
/// First instant past the domain.
const DOMAIN_END: i64 = 64;
/// Seeds the property runs over.
const SEEDS: u64 = 300;

/// A seeded SplitMix64 generator.
struct Random {
    state: u64,
}

impl Random {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    fn between(&mut self, low: i64, high: i64) -> i64 {
        low + i64::try_from(self.below(high.abs_diff(low))).expect("the domain is small")
    }

    fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }
}

fn spans(random: &mut Random, count: u64) -> Vec<TimeIntervalEvent<()>> {
    (0..random.below(count + 1))
        .map(|_| {
            let start = random.between(DOMAIN_START, DOMAIN_END - 1);
            let longest = if random.chance(1, 8) {
                DOMAIN_END - start
            } else {
                8.min(DOMAIN_END - start)
            };
            TimeIntervalEvent::raw(start, start + random.between(1, longest + 1))
        })
        .collect()
}

fn extent(random: &mut Random) -> Extent {
    if random.chance(1, 4) {
        Extent::everything()
    } else {
        Extent::from_spans(spans(random, 4))
    }
}

fn timeline(random: &mut Random, label: &str) -> TimeIntervalSequence {
    let count = random.below(8);
    TimeIntervalSequence::from_spans(label, spans(random, count)).with_observed(extent(random))
}

fn points(random: &mut Random, label: &str) -> TimePointSequence<i64> {
    let events = (0..random.below(12))
        .map(|_| {
            TimePointEvent::new(
                random.between(DOMAIN_START, DOMAIN_END),
                random.between(0, 4),
            )
        })
        .collect();
    TimePointSequence::from_events(label, events).with_observed(extent(random))
}

fn records(random: &mut Random, label: &str) -> TimePointSequence<Value> {
    let events = (0..random.below(12))
        .map(|_| {
            let mut fields = vec![("n", Value::from(random.between(0, 4)))];
            if random.chance(3, 4) {
                fields.push(("flag", Value::from(random.chance(1, 2))));
            }
            TimePointEvent::new(
                random.between(DOMAIN_START, DOMAIN_END),
                Value::record(fields),
            )
        })
        .collect();
    TimePointSequence::from_events(label, events).with_observed(extent(random))
}

fn system(random: &mut Random) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert("a", timeline(random, "a"));
    system.insert("b", timeline(random, "b"));
    system.insert("p", points(random, "p"));
    system.insert("q", points(random, "q"));
    system.insert("r", records(random, "r"));
    system
}

fn pattern(random: &mut Random) -> Pattern {
    let key = if random.chance(1, 2) { "p" } else { "q" };
    let bound = random.between(0, 4);
    let condition = if random.chance(1, 2) {
        Condition::GreaterOrEqual(bound)
    } else {
        Condition::Equals(bound)
    };
    Pattern::new(key, condition)
}

fn formula(random: &mut Random, depth: u32) -> Formula {
    let kind = if depth == 0 {
        random.below(2)
    } else {
        random.below(5)
    };
    // Half the leaves are a `Between` or a `While`.
    let kind = if kind < 2 && random.chance(1, 2) {
        5 + random.below(2)
    } else {
        kind
    };
    // A seventh of the inner nodes is a shift.
    let kind = if depth > 0 && random.chance(1, 7) {
        7
    } else {
        kind
    };
    match kind {
        0 => Formula::state(if random.chance(1, 2) { "a" } else { "b" }),
        1 => Formula::Occurs(pattern(random)),
        2 => Formula::Not(Box::new(formula(random, depth - 1))),
        3 => Formula::And(
            (0..random.below(4))
                .map(|_| formula(random, depth - 1))
                .collect(),
        ),
        5 => Formula::Between {
            start: pattern(random),
            end: pattern(random),
            open: if random.chance(1, 2) {
                OpenEnd::Extent
            } else {
                OpenEnd::Head
            },
        },
        6 => Formula::While {
            pattern: pattern(random),
            // A timeout must be positive.
            timeout: random.chance(1, 2).then(|| random.between(1, 12)),
        },
        7 => Formula::Shift {
            formula: Box::new(formula(random, depth - 1)),
            alpha: random.between(-6, 7),
            beta: random.between(-6, 7),
        },
        _ => Formula::Or(
            (0..random.below(4))
                .map(|_| formula(random, depth - 1))
                .collect(),
        ),
    }
}

/// Where one member's chunks stop: random instants of the domain, some
/// repeated, then the end of time.
///
/// Half of them are drawn from `landmarks`, the instants of the points and the
/// instants just after them, where a start, an end or a timeout can fall.
fn boundaries(random: &mut Random, landmarks: &[Timestamp]) -> Vec<Timestamp> {
    let mut stops: Vec<Timestamp> = (0..random.below(8))
        .map(|_| {
            if !landmarks.is_empty() && random.chance(1, 2) {
                let index = usize::try_from(random.below(landmarks.len() as u64))
                    .expect("the list is small");
                landmarks[index] + random.between(0, 14)
            } else {
                random.between(DOMAIN_START - 4, DOMAIN_END + 4)
            }
        })
        .collect();
    stops.sort_unstable();
    stops.push(Timestamp::MAX);
    stops
}

/// The keys `formula` reads.
fn read_keys(formula: &Formula, keys: &mut Vec<Key>) {
    match formula {
        Formula::State(key) => keys.push(key.clone()),
        Formula::Occurs(pattern) => keys.push(pattern.key.clone()),
        Formula::Between { start, end, .. } => {
            keys.push(start.key.clone());
            keys.push(end.key.clone());
        }
        Formula::While { pattern, .. } => keys.push(pattern.key.clone()),
        Formula::Not(inner) => read_keys(inner, keys),
        Formula::And(parts) | Formula::Or(parts) => {
            for part in parts {
                read_keys(part, keys);
            }
        }
        Formula::Shift { formula, .. } => read_keys(formula, keys),
    }
}

/// The last of `stops` at or before `to`, or the start of time.
fn reached(stops: &[Timestamp], to: Timestamp) -> Timestamp {
    stops
        .iter()
        .copied()
        .take_while(|&stop| stop <= to)
        .last()
        .unwrap_or(Timestamp::MIN)
}

/// A node stepped by the driver, whichever shape its chunks have.
trait Stepper {
    fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError>;
    fn frontier(&self) -> Timestamp;
}

impl Stepper for OnlineNode {
    fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        Self::step(self, inputs, to)
    }

    fn frontier(&self) -> Timestamp {
        Self::frontier(self)
    }
}

impl Stepper for OnlinePoints {
    fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        Self::step(self, inputs, to)
    }

    fn frontier(&self) -> Timestamp {
        Self::frontier(self)
    }
}

/// Step the plan over `system`, each member advancing along its own `stops`,
/// and join the chunks.
///
/// A step goes to each instant any member stops at. The members that stop
/// elsewhere hold their last stop, so the node sees inputs that reach
/// different points.
fn run_online(
    plan: &TimelinePlan,
    system: &SequenceSystem,
    read: &[Key],
    stops: &BTreeMap<Key, Vec<Timestamp>>,
) -> Result<Chunk, ProcessError> {
    drive(plan.online(), system, read, stops)
}

/// Step `node` over `system` as [`run_online`] does, and join the chunks.
fn drive(
    mut node: impl Stepper,
    system: &SequenceSystem,
    read: &[Key],
    stops: &BTreeMap<Key, Vec<Timestamp>>,
) -> Result<Chunk, ProcessError> {
    let mut joined: Option<Chunk> = None;
    let mut frontiers: BTreeMap<Key, Timestamp> = system
        .keys()
        .map(|key| (key.clone(), Timestamp::MIN))
        .collect();

    let mut steps: Vec<Timestamp> = stops.values().flatten().copied().collect();
    steps.sort_unstable();
    steps.dedup();

    for to in steps {
        let mut inputs = Inputs::new();
        for (key, frontier) in &mut frontiers {
            let target = reached(&stops[key], to);
            let sequence = system.get(key.as_str()).expect("the key is listed");
            inputs.insert(key.clone(), Chunk::of(sequence, *frontier, target));
            *frontier = target;
        }

        let before = node.frontier();
        let chunk = node.step(&inputs, to)?;
        assert_eq!(chunk.from(), before, "chunks follow each other");
        assert_eq!(
            chunk.to(),
            node.frontier(),
            "the chunk ends at the frontier"
        );
        let slowest = read.iter().map(|key| frontiers[key]).min().unwrap_or(to);
        assert!(
            node.frontier() <= slowest,
            "the output is not final beyond the slowest input"
        );
        joined = Some(match joined {
            Some(earlier) => earlier.join(chunk),
            None => chunk,
        });
    }

    Ok(joined.expect("every member stops at the end of time, so there is a step"))
}

/// [`run_online`] with every member of `system` stopping at the same `stops`.
fn run_together(
    plan: &TimelinePlan,
    system: &SequenceSystem,
    stops: &[Timestamp],
) -> Result<Chunk, ProcessError> {
    let read: Vec<Key> = system.keys().cloned().collect();
    let stops = read
        .iter()
        .map(|key| (key.clone(), stops.to_vec()))
        .collect();
    run_online(plan, system, &read, &stops)
}

/// Every member of `system` with its own stops.
fn stops_of(random: &mut Random, system: &SequenceSystem) -> BTreeMap<Key, Vec<Timestamp>> {
    let landmarks: Vec<Timestamp> = system
        .keys()
        .filter_map(|key| match system.get(key.as_str()) {
            Some(Sequence::TimePoint(points)) => {
                Some(points.instants_in(Timestamp::MIN, Timestamp::MAX))
            }
            _ => None,
        })
        .flatten()
        .collect();
    system
        .keys()
        .map(|key| (key.clone(), boundaries(random, &landmarks)))
        .collect()
}

fn bounds(timeline: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    timeline.iter().map(TimeIntervalEvent::bounds).collect()
}

#[test]
fn joined_online_chunks_equal_batch() {
    for seed in 0..SEEDS {
        let mut random = Random::new(seed);
        let system = system(&mut random);
        let formula = formula(&mut random, 3);
        let stops = stops_of(&mut random, &system);
        let mut read = Vec::new();
        read_keys(&formula, &mut read);

        let plan = TimelinePlan::compile(&formula);
        let expected = plan.batch(&system).expect("the formula reads known keys");
        let joined =
            run_online(&plan, &system, &read, &stops).expect("the formula reads known keys");

        assert_eq!(joined.from(), Timestamp::MIN, "seed {seed}: {formula}");
        assert_eq!(joined.to(), Timestamp::MAX, "seed {seed}: {formula}");
        let online = joined
            .timeline()
            .expect("a timeline formula gives timelines");
        assert_eq!(bounds(online), bounds(&expected), "seed {seed}: {formula}");
        assert_eq!(
            online.observed(),
            expected.observed(),
            "seed {seed}: {formula}"
        );
    }
}

/// A `Between` or `While` of its own, so that every seed puts one to the test.
fn opening_node(random: &mut Random) -> Formula {
    loop {
        let candidate = formula(random, 1);
        if matches!(candidate, Formula::Between { .. } | Formula::While { .. }) {
            return candidate;
        }
    }
}

#[test]
fn joined_online_chunks_of_between_and_while_equal_batch() {
    for seed in 0..SEEDS {
        let mut random = Random::new(seed ^ 0xB7);
        let system = system(&mut random);
        let formula = opening_node(&mut random);
        let stops = stops_of(&mut random, &system);
        let mut read = Vec::new();
        read_keys(&formula, &mut read);

        let plan = TimelinePlan::compile(&formula);
        let expected = plan.batch(&system).expect("the formula reads known keys");
        let joined =
            run_online(&plan, &system, &read, &stops).expect("the formula reads known keys");

        assert_eq!(joined.from(), Timestamp::MIN, "seed {seed}: {formula}");
        assert_eq!(joined.to(), Timestamp::MAX, "seed {seed}: {formula}");
        let online = joined.timeline().expect("a timeline");
        assert_eq!(bounds(online), bounds(&expected), "seed {seed}: {formula}");
        assert_eq!(
            online.observed(),
            expected.observed(),
            "seed {seed}: {formula}"
        );
    }
}

fn events(random: &mut Random) -> Events {
    match random.below(4) {
        0 => Events::Edges(formula(random, 2)),
        1 => Events::Matching(pattern(random)),
        2 => Events::changes(if random.chance(1, 2) { "p" } else { "q" }),
        _ => match random.below(3) {
            0 => Events::project("r", "n", Kind::Integer),
            1 => Events::project("r", "flag", Kind::Bool),
            _ => Events::project("r", "n", Kind::Value),
        },
    }
}

fn events_keys(events: &Events) -> Vec<Key> {
    let mut keys = Vec::new();
    match events {
        Events::Edges(formula) => read_keys(formula, &mut keys),
        Events::Matching(pattern) => keys.push(pattern.key.clone()),
        Events::Changes { key } | Events::Project { key, .. } => keys.push(key.clone()),
    }
    keys
}

#[test]
fn joined_online_points_equal_batch() {
    for seed in 0..SEEDS {
        let mut random = Random::new(seed ^ 0xE7);
        let system = system(&mut random);
        let events = events(&mut random);
        let stops = stops_of(&mut random, &system);
        let read = events_keys(&events);

        let plan = PointsPlan::compile(&events);
        let expected = plan.batch(&system).expect("the events read known keys");
        let joined =
            drive(plan.online(), &system, &read, &stops).expect("the events read known keys");

        assert_eq!(joined.from(), Timestamp::MIN, "seed {seed}: {events}");
        assert_eq!(joined.to(), Timestamp::MAX, "seed {seed}: {events}");
        let Sequence::TimePoint(online) = joined.sequence() else {
            panic!("seed {seed}: {events} gives points");
        };
        assert_eq!(online, &expected, "seed {seed}: {events}");
    }
}

/// A `Shift` of its own over a random formula, so that every seed puts one to
/// the test.
fn shifted(random: &mut Random) -> Formula {
    Formula::Shift {
        formula: Box::new(formula(random, 2)),
        alpha: random.between(-8, 9),
        beta: random.between(-8, 9),
    }
}

#[test]
fn joined_online_chunks_of_shift_equal_batch() {
    for seed in 0..SEEDS {
        let mut random = Random::new(seed ^ 0x5F);
        let system = system(&mut random);
        let formula = shifted(&mut random);
        let stops = stops_of(&mut random, &system);
        let mut read = Vec::new();
        read_keys(&formula, &mut read);

        let plan = TimelinePlan::compile(&formula);
        let expected = plan.batch(&system).expect("the formula reads known keys");
        let joined =
            run_online(&plan, &system, &read, &stops).expect("the formula reads known keys");

        assert_eq!(joined.from(), Timestamp::MIN, "seed {seed}: {formula}");
        assert_eq!(joined.to(), Timestamp::MAX, "seed {seed}: {formula}");
        let online = joined.timeline().expect("a timeline");
        assert_eq!(bounds(online), bounds(&expected), "seed {seed}: {formula}");
        assert_eq!(
            online.observed(),
            expected.observed(),
            "seed {seed}: {formula}"
        );
    }
}

#[test]
fn a_look_ahead_shift_lags_its_input() {
    let mut system = SequenceSystem::new();
    system.insert(
        "a",
        TimeIntervalSequence::from_spans("a", vec![TimeIntervalEvent::raw(10, 20)]),
    );
    let plan = TimelinePlan::compile(&Formula::state("a").shift(-5, -3));
    let mut node = plan.online();

    let mut inputs = Inputs::new();
    let timeline = system.get("a").expect("`a` is known");
    inputs.insert("a".into(), Chunk::of(timeline, Timestamp::MIN, 30));
    let chunk = node.step(&inputs, 30).expect("`a` is known");

    // An instant reads the input up to five after it.
    assert_eq!(node.frontier(), 25);
    assert_eq!(chunk.to(), 25);
    assert_eq!(bounds(chunk.timeline().expect("a timeline")), [(5, 17)]);
}

fn between_system() -> SequenceSystem {
    let mut system = SequenceSystem::new();
    let events = vec![
        TimePointEvent::new(2, 1),
        TimePointEvent::new(5, 2),
        TimePointEvent::new(9, 1),
    ];
    system.insert("p", TimePointSequence::from_events("p", events));
    system
}

fn between_formula(open: OpenEnd) -> Formula {
    Formula::Between {
        start: Pattern::new("p", Condition::Equals(1)),
        end: Pattern::new("p", Condition::Equals(2)),
        open,
    }
}

#[test]
fn a_span_open_across_chunks_closes_at_its_end() {
    let system = between_system();
    for open in [OpenEnd::Extent, OpenEnd::Head] {
        let plan = TimelinePlan::compile(&between_formula(open));
        let expected = plan.batch(&system).expect("`p` is known");
        // Cuts exactly at the start, at the end and just after.
        for stops in [vec![2, 5, 6, 9, 10], vec![3, 4, 5], vec![9]] {
            let mut stops = stops;
            stops.push(Timestamp::MAX);
            let joined = run_together(&plan, &system, &stops).expect("`p` is known");
            let online = joined.timeline().expect("a timeline");
            assert_eq!(bounds(online), bounds(&expected), "{open:?} {stops:?}");
            assert_eq!(online.observed(), expected.observed(), "{open:?} {stops:?}");
        }
    }
}

#[test]
fn a_head_that_has_not_moved_holds_the_output_back() {
    let system = between_system();
    let plan = TimelinePlan::compile(&between_formula(OpenEnd::Head));
    let mut node = plan.online();

    let mut inputs = Inputs::new();
    let points = system.get("p").expect("`p` is known");
    inputs.insert("p".into(), Chunk::of(points, Timestamp::MIN, 20));
    node.step(&inputs, 20).expect("`p` is known");

    // The last point is at 9, so what lies after 10 may still be observed.
    assert_eq!(node.frontier(), 10);
}

#[test]
fn a_timeout_cut_at_a_chunk_boundary_is_kept() {
    let system = between_system();
    let plan = TimelinePlan::compile(&Formula::While {
        pattern: Pattern::new("p", Condition::Equals(1)),
        timeout: Some(3),
    });
    let expected = plan.batch(&system).expect("`p` is known");
    // The sample at 2 times out at 5, the one at 9 at 12.
    for stops in [vec![5], vec![4, 5, 6], vec![12, 13], vec![3, 11]] {
        let mut stops = stops;
        stops.push(Timestamp::MAX);
        let joined = run_together(&plan, &system, &stops).expect("`p` is known");
        let online = joined.timeline().expect("a timeline");
        assert_eq!(bounds(online), bounds(&expected), "{stops:?}");
        assert_eq!(online.observed(), expected.observed(), "{stops:?}");
    }
}

#[test]
fn a_span_cut_at_a_boundary_merges_back() {
    let mut system = SequenceSystem::new();
    system.insert(
        "up",
        TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::raw(0, 10)]),
    );
    let plan = TimelinePlan::compile(&Formula::state("up"));

    let joined = run_together(&plan, &system, &[4, 7, 7, Timestamp::MAX]).expect("`up` is known");

    let timeline = joined.timeline().expect("a timeline");
    assert_eq!(bounds(timeline), [(0, 10)]);
}

#[test]
fn a_missing_or_misshapen_member_is_refused() {
    let mut system = SequenceSystem::new();
    system.insert("p", points(&mut Random::new(1), "p"));

    let missing = TimelinePlan::compile(&Formula::state("up"));
    let outcome = run_together(&missing, &system, &[Timestamp::MAX]);
    assert!(matches!(outcome, Err(ProcessError::UnknownKey { .. })));

    let misshapen = TimelinePlan::compile(&Formula::state("p"));
    let outcome = run_together(&misshapen, &system, &[Timestamp::MAX]);
    assert!(matches!(outcome, Err(ProcessError::NotATimeline { .. })));

    let reading_a_timeline = TimelinePlan::compile(&Formula::Occurs(Pattern::new(
        "p",
        Condition::Equals("text".to_owned()),
    )));
    let outcome = run_together(&reading_a_timeline, &system, &[Timestamp::MAX]);
    assert!(matches!(
        outcome,
        Err(ProcessError::ValueTypeMismatch { .. })
    ));
}

fn point_chunk(from: i64, to: i64, events: Vec<(i64, i64)>) -> Chunk {
    let events = events
        .into_iter()
        .map(|(instant, value)| TimePointEvent::new(instant, value))
        .collect();
    Chunk::new(
        from,
        to,
        TimePointSequence::from_events("p", events)
            .with_observed(Extent::everything())
            .into(),
    )
}

#[test]
fn joined_point_chunks_hold_every_event_in_order() {
    let joined = point_chunk(0, 5, vec![(1, 7), (4, 8)]).join(point_chunk(5, 9, vec![(5, 9)]));
    assert_eq!((joined.from(), joined.to()), (0, 9));
    assert_eq!(joined.sequence().len(), 3);
}

#[test]
#[should_panic(expected = "chunks join end to end")]
fn chunks_with_a_gap_do_not_join() {
    let _ = point_chunk(0, 5, vec![]).join(point_chunk(6, 9, vec![]));
}

#[test]
#[should_panic(expected = "chunks of one output carry one value type")]
fn point_chunks_of_different_value_types_do_not_join() {
    let text = Chunk::new(
        5,
        9,
        TimePointSequence::from_events("p", vec![TimePointEvent::new(6, "a".to_owned())]).into(),
    );
    let _ = point_chunk(0, 5, vec![(1, 1)]).join(text);
}

#[test]
#[should_panic(expected = "chunks of one output have one shape")]
fn chunks_of_different_shapes_do_not_join() {
    let timeline = Chunk::new(5, 9, TimeIntervalSequence::from_spans("t", vec![]).into());
    let _ = point_chunk(0, 5, vec![]).join(timeline);
}

#[test]
fn slicing_a_chunk_stays_inside_its_range() {
    let chunk = point_chunk(2, 8, vec![(2, 1), (5, 2), (7, 3)]);
    let cut = chunk.slice(0, 6);
    assert_eq!((cut.from(), cut.to()), (2, 6));
    assert_eq!(cut.sequence().len(), 2);

    let empty = chunk.slice(20, 30);
    assert!(empty.to() >= empty.from());
    assert_eq!(empty.sequence().len(), 0);
}

fn extent_of(start: i64, end: i64) -> Extent {
    Extent::from_spans(vec![TimeIntervalEvent::raw(start, end)])
}

#[test]
fn appending_points_sets_the_extent_to_the_union_of_both() {
    let first = TimePointSequence::from_events("p", vec![TimePointEvent::new(2, 1_i64)])
        .with_observed(extent_of(0, 5));
    // A gap separates the two extents: it stays unobserved.
    let second = TimePointSequence::from_events("p", vec![TimePointEvent::new(9, 2_i64)])
        .with_observed(extent_of(8, 10));

    let mut joined = AnyPointSequence::from(first);
    assert!(joined.append(&AnyPointSequence::from(second)));

    assert_eq!(
        joined.observed(),
        &Extent::from_spans(vec![
            TimeIntervalEvent::raw(0, 5),
            TimeIntervalEvent::raw(8, 10)
        ])
    );
    assert_eq!(joined.len(), 2);
}

#[test]
fn appending_points_of_another_value_type_changes_nothing() {
    let first = TimePointSequence::from_events("p", vec![TimePointEvent::new(2, 1_i64)])
        .with_observed(extent_of(0, 5));
    let other = TimePointSequence::from_events("p", vec![TimePointEvent::new(7, true)])
        .with_observed(extent_of(5, 10));

    let mut joined = AnyPointSequence::from(first);
    let before = joined.clone();
    assert!(!joined.append(&AnyPointSequence::from(other)));
    assert_eq!(joined.observed(), before.observed());
    assert_eq!(joined.len(), 1);
}

#[test]
fn edges_stepped_in_chunks_have_no_edge_at_a_cut_and_forget_a_closed_span() {
    let mut system = SequenceSystem::new();
    system.insert(
        "a",
        TimeIntervalSequence::from_spans(
            "a",
            vec![
                TimeIntervalEvent::raw(10, 20),
                TimeIntervalEvent::raw(40, 50),
            ],
        ),
    );
    let plan = PointsPlan::compile(&Events::Edges(Formula::state("a")));
    let mut node = plan.online();
    let timeline = system.get("a").expect("`a` is known");

    let mut stamps = Vec::new();
    // Cut inside the first span, after it, inside the second, after it.
    for to in [15, 30, 45, 60] {
        let mut inputs = Inputs::new();
        inputs.insert("a".into(), Chunk::of(timeline, Timestamp::MIN, to));
        let chunk = node.step(&inputs, to).expect("`a` is known");
        let Sequence::TimePoint(points) = chunk.sequence() else {
            panic!("edges give points");
        };
        let points = points
            .downcast_ref::<crate::processing::Edge>()
            .expect("edges");
        stamps.extend(
            points
                .iter()
                .map(|event| (event.timestamp(), *event.value())),
        );
    }

    use crate::processing::Edge::{End, Start};
    assert_eq!(stamps, [(10, Start), (20, End), (40, Start), (50, End)]);
}

/// A pattern on a field of the records `r`.
fn field_pattern(random: &mut Random) -> Pattern {
    if random.chance(1, 4) {
        Pattern::field(
            "r",
            "flag",
            Condition::Equals(Value::from(random.chance(1, 2))),
        )
    } else {
        Pattern::field(
            "r",
            "n",
            Condition::GreaterOrEqual(Value::from(random.between(0, 4))),
        )
    }
}

/// The formula of one entity: a node over its own records, alone or combined
/// with a formula over the other members.
fn entity_formula(random: &mut Random) -> Formula {
    let own = match random.below(3) {
        0 => Formula::Occurs(field_pattern(random)),
        1 => Formula::While {
            pattern: field_pattern(random),
            timeout: random.chance(1, 2).then(|| random.between(1, 12)),
        },
        _ => Formula::Between {
            start: field_pattern(random),
            end: field_pattern(random),
            open: if random.chance(1, 2) {
                OpenEnd::Extent
            } else {
                OpenEnd::Head
            },
        },
    };
    match random.below(5) {
        0 => own,
        1 => Formula::And(vec![own, formula(random, 1)]),
        2 => Formula::Or(vec![own, formula(random, 1)]),
        3 => Formula::Not(Box::new(own)),
        _ => Formula::Shift {
            formula: Box::new(own),
            alpha: random.between(-6, 7),
            beta: random.between(-6, 7),
        },
    }
}

fn entity_recipe(random: &mut Random) -> Recipe {
    let by = Group::new("r", if random.chance(3, 4) { "n" } else { "flag" });
    let formula = entity_formula(random);
    match random.below(3) {
        0 => Recipe::per_entity(by, formula),
        1 => Recipe::any_entity(by, formula),
        _ => Recipe::count_entities(by, formula),
    }
}

fn recipe_keys(recipe: &Recipe) -> Vec<Key> {
    let (Recipe::PerEntity { by, formula }
    | Recipe::AnyEntity { by, formula }
    | Recipe::CountEntities { by, formula }) = recipe
    else {
        panic!("an entity recipe");
    };
    let mut keys = vec![by.key.clone()];
    read_keys(formula, &mut keys);
    keys
}

/// Step a recipe over `system` as [`drive`] does, and join the chunks of each
/// member, by the name of its entity.
fn drive_recipe(
    mut node: OnlineRecipe,
    system: &SequenceSystem,
    read: &[Key],
    stops: &BTreeMap<Key, Vec<Timestamp>>,
) -> Result<BTreeMap<String, Chunk>, ProcessError> {
    let mut joined: BTreeMap<String, Chunk> = BTreeMap::new();
    let mut frontiers: BTreeMap<Key, Timestamp> = system
        .keys()
        .map(|key| (key.clone(), Timestamp::MIN))
        .collect();

    let mut steps: Vec<Timestamp> = stops.values().flatten().copied().collect();
    steps.sort_unstable();
    steps.dedup();

    for to in steps {
        let mut inputs = Inputs::new();
        for (key, frontier) in &mut frontiers {
            let target = reached(&stops[key], to);
            let sequence = system.get(key.as_str()).expect("the key is listed");
            inputs.insert(key.clone(), Chunk::of(sequence, *frontier, target));
            *frontier = target;
        }

        for member in node.step(&inputs, to)? {
            let name = member.entity.as_ref().map(render).unwrap_or_default();
            let chunk = member.chunk;
            if let Some(earlier) = joined.remove(&name) {
                joined.insert(name, earlier.join(chunk));
            } else {
                joined.insert(name, chunk);
            }
        }
        let slowest = read.iter().map(|key| frontiers[key]).min().unwrap_or(to);
        assert!(
            node.frontier() <= slowest,
            "the output is not final beyond the slowest input"
        );
        for chunk in joined.values() {
            assert_eq!(
                chunk.to(),
                node.frontier(),
                "every member is at the frontier"
            );
        }
    }

    Ok(joined)
}

#[test]
fn joined_online_entity_chunks_equal_batch() {
    for seed in 0..SEEDS {
        let mut random = Random::new(seed ^ 0xE1);
        let system = system(&mut random);
        let recipe = entity_recipe(&mut random);
        let stops = stops_of(&mut random, &system);
        let read = recipe_keys(&recipe);
        let output = Key::from("out");

        let plan = compile_recipe(&recipe);
        let joined = drive_recipe(plan.online(&output), &system, &read, &stops)
            .expect("the recipe reads known keys");

        match &plan {
            RecipePlan::PerEntity(_) => {
                let expected = plan
                    .members(&output, &system)
                    .expect("the recipe reads known keys");
                assert_eq!(joined.len(), expected.len(), "seed {seed}: {recipe}");
                for member in expected {
                    let name = render(&member.entity.expect("a per-entity member"));
                    let chunk = &joined[&name];
                    assert_eq!(chunk.from(), Timestamp::MIN, "seed {seed}: {recipe}");
                    assert_eq!(chunk.to(), Timestamp::MAX, "seed {seed}: {recipe}");
                    let online = chunk.timeline().expect("a timeline");
                    let expected = member.sequence.as_time_interval().expect("a timeline");
                    assert_eq!(bounds(online), bounds(expected), "seed {seed}: {recipe}");
                    assert_eq!(
                        online.observed(),
                        expected.observed(),
                        "seed {seed}: {recipe}"
                    );
                }
            }
            _ => {
                let expected = plan.batch(&system).expect("the recipe reads known keys");
                let chunk = &joined[""];
                assert_eq!(joined.len(), 1, "seed {seed}: {recipe}");
                assert_eq!(chunk.from(), Timestamp::MIN, "seed {seed}: {recipe}");
                assert_eq!(chunk.to(), Timestamp::MAX, "seed {seed}: {recipe}");
                match (chunk.sequence(), &expected) {
                    (Sequence::TimeInterval(online), Sequence::TimeInterval(expected)) => {
                        assert_eq!(bounds(online), bounds(expected), "seed {seed}: {recipe}");
                        assert_eq!(
                            online.observed(),
                            expected.observed(),
                            "seed {seed}: {recipe}"
                        );
                    }
                    (Sequence::TimePoint(online), Sequence::TimePoint(expected)) => {
                        assert_eq!(online, expected, "seed {seed}: {recipe}");
                    }
                    _ => panic!("seed {seed}: {recipe} gives the shape batch gives"),
                }
            }
        }
    }
}

#[test]
fn an_entity_seen_mid_stream_is_unobserved_before_its_first_record() {
    let mut system = SequenceSystem::new();
    system.insert(
        "r",
        TimePointSequence::from_events(
            "r",
            vec![
                TimePointEvent::new(2, Value::record(vec![("n", Value::from(1_i64))])),
                TimePointEvent::new(20, Value::record(vec![("n", Value::from(2_i64))])),
            ],
        )
        .with_observed(Extent::everything()),
    );
    let by = Group::new("r", "n");
    let formula = Formula::Occurs(Pattern::field(
        "r",
        "n",
        Condition::GreaterOrEqual(Value::from(0_i64)),
    ));
    let plan = compile_recipe(&Recipe::per_entity(by, formula));
    let mut node = plan.online(&Key::from("out"));

    let mut inputs = Inputs::new();
    let records = system.get("r").expect("`r` is known");
    inputs.insert("r".into(), Chunk::of(records, Timestamp::MIN, 30));
    let members = node.step(&inputs, 30).expect("`r` is known");

    assert_eq!(members.len(), 2);
    let second = members[1].chunk.timeline().expect("a timeline");
    assert_eq!(bounds(second), [(20, 21)]);
    assert!(!second.observed().as_slice().is_empty());
    assert_eq!(second.observed().as_slice()[0].bounds(), (20, 30));
}

#[test]
fn entities_that_share_a_name_are_refused() {
    let mut system = SequenceSystem::new();
    system.insert(
        "r",
        TimePointSequence::from_events(
            "r",
            vec![
                TimePointEvent::new(2, Value::record(vec![("n", Value::from(1_i64))])),
                TimePointEvent::new(3, Value::record(vec![("n", Value::from("1"))])),
            ],
        )
        .with_observed(Extent::everything()),
    );
    let by = Group::new("r", "n");
    let formula = Formula::state("a");
    let plan = compile_recipe(&Recipe::per_entity(by, formula));
    let mut node = plan.online(&Key::from("out"));

    let mut inputs = Inputs::new();
    inputs.insert(
        "r".into(),
        Chunk::of(system.get("r").expect("`r` is known"), Timestamp::MIN, 30),
    );
    inputs.insert(
        "a".into(),
        Chunk::new(
            Timestamp::MIN,
            30,
            Sequence::from(TimeIntervalSequence::from_spans("a", Vec::new())),
        ),
    );

    assert!(matches!(
        node.step(&inputs, 30),
        Err(ProcessError::DuplicateMember { .. })
    ));
}
