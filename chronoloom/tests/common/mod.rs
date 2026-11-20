//! Shared harness of the integration tests: a seeded generator, generators of
//! random inputs, and an oracle to check timelines against.
//!
//! The crate has no dependencies, not even for tests, so this is written here
//! rather than taken from `proptest`. Inputs come from a small domain of
//! instants, [`DOMAIN_START`] to [`DOMAIN_END`], so that collisions and
//! touching spans are frequent. A property runs over many seeds through
//! [`for_each_seed`], and a failure names its seed so it can be replayed alone.

use std::collections::BTreeSet;

use chronoloom::{TimeIntervalEvent, TimePointEvent};

mod knowledge;
mod random;

pub use knowledge::{assert_knows, observed_instants, Knowledge, Truth};
pub use random::{bounds, extent, for_each_seed, span, spans, Random, DOMAIN_END, DOMAIN_START};

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
