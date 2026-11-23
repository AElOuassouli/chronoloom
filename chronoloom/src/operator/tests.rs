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
use crate::processing::{Condition, Formula, Pattern, ProcessError};
use crate::sequence_system::{Key, SequenceSystem};
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};

use super::chunk::{Chunk, Inputs};
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

fn system(random: &mut Random) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    system.insert("a", timeline(random, "a"));
    system.insert("b", timeline(random, "b"));
    system.insert("p", points(random, "p"));
    system.insert("q", points(random, "q"));
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
    match kind {
        0 => Formula::state(if random.chance(1, 2) { "a" } else { "b" }),
        1 => Formula::Occurs(pattern(random)),
        2 => Formula::Not(Box::new(formula(random, depth - 1))),
        3 => Formula::And(
            (0..random.below(4))
                .map(|_| formula(random, depth - 1))
                .collect(),
        ),
        _ => Formula::Or(
            (0..random.below(4))
                .map(|_| formula(random, depth - 1))
                .collect(),
        ),
    }
}

/// Where one member's chunks stop: random instants of the domain, some
/// repeated, then the end of time.
fn boundaries(random: &mut Random) -> Vec<Timestamp> {
    let mut stops: Vec<Timestamp> = (0..random.below(8))
        .map(|_| random.between(DOMAIN_START - 4, DOMAIN_END + 4))
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
        Formula::Not(inner) => read_keys(inner, keys),
        Formula::And(parts) | Formula::Or(parts) => {
            for part in parts {
                read_keys(part, keys);
            }
        }
        _ => unreachable!("the generator draws no other kind"),
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
    let mut node = plan.online().expect("every node of the formula is online");
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
    system
        .keys()
        .map(|key| (key.clone(), boundaries(random)))
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

#[test]
fn kinds_without_an_online_form_say_so() {
    let plan = TimelinePlan::compile(&Formula::state("up").shift(1, 2));
    assert!(plan.online().is_none());
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
