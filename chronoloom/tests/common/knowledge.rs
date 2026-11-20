//! The three-valued oracle: what is known of each instant of a timeline that
//! may not have been observed everywhere.

use chronoloom::{Extent, TimeIntervalEvent, TimeIntervalSequence};

use super::{bounds, span, InstantSet, DOMAIN_END, DOMAIN_START};

/// First instant the three-valued oracle knows anything about.
pub const CHECK_START: i64 = DOMAIN_START - 20;

/// First instant past the range the three-valued oracle knows about.
pub const CHECK_END: i64 = DOMAIN_END + 20;

/// What is known of one instant: active, inactive, or not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    /// Observed, and active.
    Active,
    /// Observed, and not active.
    Inactive,
    /// Not observed, so nothing is known.
    Unknown,
}

impl Truth {
    /// Kleene's "or": active as soon as either side is.
    pub fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Active, _) | (_, Self::Active) => Self::Active,
            (Self::Inactive, Self::Inactive) => Self::Inactive,
            _ => Self::Unknown,
        }
    }

    /// Kleene's "and": inactive as soon as either side is.
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Inactive, _) | (_, Self::Inactive) => Self::Inactive,
            (Self::Active, Self::Active) => Self::Active,
            _ => Self::Unknown,
        }
    }

    /// Negation: unknown stays unknown.
    pub fn not(self) -> Self {
        match self {
            Self::Active => Self::Inactive,
            Self::Inactive => Self::Active,
            Self::Unknown => Self::Unknown,
        }
    }

    /// Exclusive or: decided only when both sides are.
    pub fn exclusive_or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (left, right) if left == right => Self::Inactive,
            _ => Self::Active,
        }
    }
}

/// The instants observed and the instants active, over the range
/// [`CHECK_START`] to [`CHECK_END`].
///
/// Every operation is defined by the truth value of each instant, so none of
/// them restates the extent formulas the library uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knowledge {
    /// The instants whose state is known.
    pub observed: InstantSet,
    /// The instants known active, all of them observed.
    pub active: InstantSet,
}

impl Knowledge {
    /// What is known of `active` observed over `observed`, the active instants
    /// cut to what was observed.
    pub fn new(observed: InstantSet, active: &InstantSet) -> Self {
        Self {
            active: active.intersection(&observed),
            observed,
        }
    }

    /// The state of `instant`.
    pub fn state(&self, instant: i64) -> Truth {
        if self.active.contains(instant) {
            Truth::Active
        } else if self.observed.contains(instant) {
            Truth::Inactive
        } else {
            Truth::Unknown
        }
    }

    /// The knowledge whose instants are given by `state`.
    pub fn from_states(state: impl Fn(i64) -> Truth) -> Self {
        let mut observed = Vec::new();
        let mut active = Vec::new();
        for instant in CHECK_START..CHECK_END {
            match state(instant) {
                Truth::Active => {
                    observed.push(span(instant, instant + 1));
                    active.push(span(instant, instant + 1));
                }
                Truth::Inactive => observed.push(span(instant, instant + 1)),
                Truth::Unknown => {}
            }
        }
        Self {
            observed: InstantSet::from_spans(&observed),
            active: InstantSet::from_spans(&active),
        }
    }

    fn combine(&self, other: &Self, rule: impl Fn(Truth, Truth) -> Truth) -> Self {
        Self::from_states(|instant| rule(self.state(instant), other.state(instant)))
    }

    /// Union, as Kleene's "or".
    pub fn union(&self, other: &Self) -> Self {
        self.combine(other, Truth::or)
    }

    /// Intersection, as Kleene's "and".
    pub fn intersection(&self, other: &Self) -> Self {
        self.combine(other, Truth::and)
    }

    /// Difference, as "and not".
    pub fn difference(&self, other: &Self) -> Self {
        self.combine(other, |left, right| left.and(right.not()))
    }

    /// Symmetric difference, as exclusive or.
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        self.combine(other, Truth::exclusive_or)
    }

    /// Complement, as negation.
    pub fn complement(&self) -> Self {
        Self::from_states(|instant| self.state(instant).not())
    }

    /// The transform, decided from the window of instants each result instant
    /// reads.
    ///
    /// A widening transform (`alpha <= beta`) is active when the window
    /// `[u - beta, u - alpha]` holds an active instant, and decided inactive
    /// when all of it is observed. A narrowing one is active when the window
    /// `[u - alpha, u - beta]` is all active, and decided inactive when it
    /// holds an instant known inactive. Instants past the range are unknown.
    pub fn transform(&self, alpha: i64, beta: i64) -> Self {
        let (low, high) = (alpha.min(beta), alpha.max(beta));
        let widening = alpha <= beta;
        Self::from_states(|instant| {
            let window: Vec<Truth> = ((instant - high)..=(instant - low))
                .map(|read| {
                    if (CHECK_START..CHECK_END).contains(&read) {
                        self.state(read)
                    } else {
                        Truth::Unknown
                    }
                })
                .collect();
            let any = |wanted: Truth| window.contains(&wanted);
            let all = |wanted: Truth| window.iter().all(|&state| state == wanted);
            if widening {
                if any(Truth::Active) {
                    Truth::Active
                } else if !any(Truth::Unknown) {
                    Truth::Inactive
                } else {
                    Truth::Unknown
                }
            } else if all(Truth::Active) {
                Truth::Active
            } else if any(Truth::Inactive) {
                Truth::Inactive
            } else {
                Truth::Unknown
            }
        })
    }
}

/// Check `timeline` against `expected`: the same active spans, and the same
/// instants observed, on every instant the oracle can decide.
pub fn assert_knows(timeline: &TimeIntervalSequence, expected: &Knowledge, context: &str) {
    // An extent of everything runs past the range the oracle knows about.
    let within_range: Vec<(i64, i64)> = bounds(timeline)
        .into_iter()
        .map(|(start, end)| (start.max(CHECK_START), end.min(CHECK_END)))
        .filter(|(start, end)| start < end)
        .collect();
    assert_eq!(
        within_range,
        expected.active.to_spans(),
        "active spans, {context}"
    );
    // Far enough inside the range that a window never reaches past it.
    for instant in (CHECK_START + 12)..(CHECK_END - 12) {
        assert_eq!(
            timeline.is_observed(instant),
            expected.state(instant) != Truth::Unknown,
            "is_observed({instant}), {context}"
        );
        if timeline.contains(instant) {
            assert!(
                timeline.is_observed(instant),
                "active but unobserved at {instant}, {context}"
            );
        }
    }
}

/// The oracle's observed set for `extent`, within the checked range.
pub fn observed_instants(extent: &Extent) -> InstantSet {
    let range = span(CHECK_START, CHECK_END);
    let inside: Vec<TimeIntervalEvent<()>> = extent
        .as_slice()
        .iter()
        .filter_map(|run| run.intersection(&range))
        .collect();
    InstantSet::from_spans(&inside)
}
