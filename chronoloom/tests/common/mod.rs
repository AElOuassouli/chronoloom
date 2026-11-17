//! Shared harness of the integration tests: a seeded generator, generators of
//! random inputs, and an oracle to check timelines against.
//!
//! The crate has no dependencies, not even for tests, so this is written here
//! rather than taken from `proptest`. Inputs come from a small domain of
//! instants, [`DOMAIN_START`] to [`DOMAIN_END`], so that collisions and
//! touching spans are frequent. A property runs over many seeds through
//! [`for_each_seed`], and a failure names its seed so it can be replayed alone.

use std::collections::BTreeSet;
use std::panic::{self, AssertUnwindSafe};

use chronoloom::{TimeIntervalEvent, TimeIntervalSequence, TimePointEvent};

/// First instant generated inputs can use.
pub const DOMAIN_START: i64 = 0;

/// First instant past the end of the domain: generated inputs stay below it.
pub const DOMAIN_END: i64 = 64;

/// A seeded SplitMix64 generator: the same seed always gives the same draws.
pub struct Random {
    state: u64,
}

impl Random {
    /// A generator whose draws are fixed by `seed`.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A number in `0..bound`.
    ///
    /// # Panics
    ///
    /// Panics if `bound` is zero.
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "an empty range has nothing to draw");
        self.next_u64() % bound
    }

    /// A number in `low..high`.
    ///
    /// # Panics
    ///
    /// Panics if the range is empty.
    pub fn between(&mut self, low: i64, high: i64) -> i64 {
        assert!(low < high, "an empty range has nothing to draw");
        let width = high.abs_diff(low);
        low.wrapping_add_unsigned(self.below(width))
    }

    /// True with probability `numerator / denominator`.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }
}

/// Run `property` once per seed in `0..cases`, naming the seed that fails.
///
/// The property receives a generator fixed by the seed, and the seed itself.
///
/// # Panics
///
/// Panics with the failing seed and the original message if the property
/// panics for any seed.
pub fn for_each_seed(cases: u64, mut property: impl FnMut(&mut Random, u64)) {
    for seed in 0..cases {
        let mut random = Random::new(seed);
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| property(&mut random, seed)));
        if let Err(payload) = outcome {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("(no message)");
            panic!("property failed for seed {seed}: {message}");
        }
    }
}

/// A span, for tests where the bounds are known good.
pub fn span(start: i64, end: i64) -> TimeIntervalEvent<()> {
    TimeIntervalEvent::span(start, end).expect("test bounds are ordered")
}

/// Up to `count` random spans inside the domain, in no particular order,
/// overlapping and touching often.
pub fn spans(random: &mut Random, count: u64) -> Vec<TimeIntervalEvent<()>> {
    let drawn = random.below(count + 1);
    (0..drawn)
        .map(|_| {
            let start = random.between(DOMAIN_START, DOMAIN_END - 1);
            // Mostly short spans, so the timeline keeps gaps; sometimes long.
            let longest = if random.chance(1, 8) {
                DOMAIN_END - start
            } else {
                8.min(DOMAIN_END - start)
            };
            let end = start + random.between(1, longest + 1);
            span(start, end)
        })
        .collect()
}

/// Up to `count` random point events inside the domain, in no particular
/// order, each carrying the order it was drawn in so ties can be told apart.
pub fn point_events(random: &mut Random, count: u64) -> Vec<TimePointEvent<u64>> {
    let drawn = random.below(count + 1);
    (0..drawn)
        .map(|order| TimePointEvent::new(random.between(DOMAIN_START, DOMAIN_END), order))
        .collect()
}

/// An instant inside the domain, or a little outside it on either side.
pub fn instants(random: &mut Random) -> i64 {
    random.between(DOMAIN_START - 4, DOMAIN_END + 4)
}

/// The oracle: a timeline as the plain set of instants it covers.
///
/// Every operation here follows its definition instant by instant, so it is
/// slow but obviously right, and a timeline is checked by comparing its spans
/// with [`InstantSet::to_spans`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstantSet {
    instants: BTreeSet<i64>,
}

impl InstantSet {
    /// The instants covered by any of `spans`.
    pub fn from_spans<'a>(spans: impl IntoIterator<Item = &'a TimeIntervalEvent<()>>) -> Self {
        let instants = spans
            .into_iter()
            .flat_map(|span| span.start()..span.end())
            .collect();
        Self { instants }
    }

    /// The number of instants covered.
    pub fn len(&self) -> u64 {
        self.instants.len() as u64
    }

    /// Whether `instant` is covered.
    pub fn contains(&self, instant: i64) -> bool {
        self.instants.contains(&instant)
    }

    /// Instants covered by either set.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            instants: self.instants.union(&other.instants).copied().collect(),
        }
    }

    /// Instants covered by both sets.
    pub fn intersection(&self, other: &Self) -> Self {
        Self {
            instants: self
                .instants
                .intersection(&other.instants)
                .copied()
                .collect(),
        }
    }

    /// Instants covered by this set and not by `other`.
    pub fn difference(&self, other: &Self) -> Self {
        Self {
            instants: self.instants.difference(&other.instants).copied().collect(),
        }
    }

    /// Instants covered by exactly one of the two sets.
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        Self {
            instants: self
                .instants
                .symmetric_difference(&other.instants)
                .copied()
                .collect(),
        }
    }

    /// The transform by its definition: `u` is covered when some maximal run
    /// `[s, e)` of this set has `s + alpha <= u < e + beta`.
    ///
    /// The runs are the set's own, not the spans it was built from: narrowing
    /// `[0, 5)` and `[5, 10)` one at a time would cut a hole at 5 that the
    /// timeline `[0, 10)` does not have.
    pub fn transform(&self, alpha: i64, beta: i64) -> Self {
        let instants = self
            .to_spans()
            .into_iter()
            .flat_map(|(start, end)| (start + alpha)..(end + beta))
            .collect();
        Self { instants }
    }

    /// The maximal runs of consecutive covered instants, as `(start, end)`
    /// bounds in time order.
    pub fn to_spans(&self) -> Vec<(i64, i64)> {
        let mut runs: Vec<(i64, i64)> = Vec::new();
        for &instant in &self.instants {
            match runs.last_mut() {
                Some((_, end)) if *end == instant => *end += 1,
                _ => runs.push((instant, instant + 1)),
            }
        }
        runs
    }
}

/// The bounds of every span of `timeline`, in order.
pub fn bounds(timeline: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    timeline.iter().map(TimeIntervalEvent::bounds).collect()
}
