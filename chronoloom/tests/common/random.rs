//! The generators of the harness, with no oracle: a seeded generator, random
//! spans and extents, and the runner of properties over seeds. Test files that
//! need nothing else include this file alone, so no oracle sits unused in them.

use std::panic::{self, AssertUnwindSafe};

use chronoloom::{Extent, TimeIntervalEvent, TimeIntervalSequence};

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

/// Seeds per property when `CHRONOLOOM_PROPERTY_CASES` is not set.
pub const LOCAL_CASES: u64 = 50;

/// How many seeds a property runs over.
///
/// Returns the value of the `CHRONOLOOM_PROPERTY_CASES` environment variable
/// when it is set (continuous integration sets it for the thorough run),
/// otherwise [`LOCAL_CASES`], which keeps the local check fast.
///
/// # Panics
///
/// Panics if the variable is set to something that is not a whole number.
pub fn property_cases() -> u64 {
    match std::env::var("CHRONOLOOM_PROPERTY_CASES") {
        Ok(value) => value
            .parse()
            .expect("CHRONOLOOM_PROPERTY_CASES is a whole number"),
        Err(_) => LOCAL_CASES,
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

/// The bounds of every span of `timeline`, in order.
pub fn bounds(timeline: &TimeIntervalSequence) -> Vec<(i64, i64)> {
    timeline.iter().map(TimeIntervalEvent::bounds).collect()
}

/// A random extent: everything one time in four, otherwise random spans
/// inside the domain, which may be none at all.
pub fn extent(random: &mut Random) -> Extent {
    if random.chance(1, 4) {
        Extent::everything()
    } else {
        Extent::from_spans(spans(random, 4))
    }
}
