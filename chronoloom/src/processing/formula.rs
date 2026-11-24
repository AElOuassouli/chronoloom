//! A state described as data: a tree of formulas that denotes a timeline.

use core::fmt;
use core::ops::Not;

use crate::primitives::Timestamp;
use crate::sequence_system::Key;
#[cfg(doc)]
use crate::sequence_system::SequenceSystem;
#[cfg(doc)]
use crate::sequences::TimeIntervalSequence;

use super::condition::Pattern;

/// Where a [`Formula::Between`] span with no end stops.
///
/// A state stays active while no end event has been observed, so the span of
/// a start that no end follows has to stop somewhere. The two options differ in
/// what they claim about the instants after the last event.
///
/// - [`Extent`](OpenEnd::Extent) stops the span at the end of the run of the
///   extent that contains its start. The formula is known active up to there
///   and unknown after, because there nothing was observed. With the default
///   extent, everything, the span stays active for good.
/// - [`Head`](OpenEnd::Head) stops it at the head, the instant up to which the
///   inputs have been seen. The formula is unknown from the head on, whether or
///   not a span is open, since an end event may still arrive there. The two
///   options agree where the extent reaches the head, which is how a live
///   stream looks.
///
/// The head of a batch is one past the latest timestamp among the points of
/// the two members the formula reads, whichever points match. A span opened at
/// `s` is `[s, max(head, s + 1))`, so it always lasts at least one tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum OpenEnd {
    /// Up to the end of the extent's run that contains the start.
    #[default]
    Extent,
    /// Up to the head, the instant up to which the inputs have been seen.
    Head,
}

/// What a state is, as a tree: every node denotes a timeline, with an extent.
///
/// Leaves read a [`SequenceSystem`] and inner nodes combine what the leaves
/// denote with the set algebra of [`TimeIntervalSequence`], so a formula is
/// data about a timeline rather than a procedure that builds one. It can be
/// compared, cloned, printed, normalized and evaluated against any system, and
/// a [`Processor`](crate::processor::Processor) is one recipe built on one.
///
/// | Variant | Active | Written |
/// |---|---|---|
/// | `State(key)` | wherever the timeline `key` is, as is | `up` |
/// | `Between { start, end, open }` | from a point matching `start` until the next one matching `end` | `between(motion = "start", motion = "end")` |
/// | `While { pattern, timeout }` | while the last sample of `pattern.key` matches (sample and hold), for at most `timeout` ticks after it if there is one | `while(count > 5)`, `while(count > 5, within 30)` |
/// | `Occurs(pattern)` | on `[t, t + 1)` for each matching point | `occurs(door = "open")` |
/// | `Not(f)` | where `f` is observed and not active | `¬f` |
/// | `And(fs)` / `Or(fs)` | where all / any of the `fs` are | `f ∧ g`, `f ∨ g` |
/// | `Shift { formula, alpha, beta }` | `formula` with every span `[s, e)` moved to `[s + alpha, e + beta)` | `f[alpha, beta]` |
///
/// A threshold (`count > 5`) is `While`, a range ("between this and that") is
/// `While` with [`Condition::Within`], and an edge stream is `Between`.
/// Holding an event for `h` ticks is `Shift { Occurs(p), 0, h - 1 }`, which
/// needs no variant of its own: [`hold`](Formula::hold) builds it, and holds
/// that overlap or touch merge into one span like any other timeline.
///
/// # Three-valued
///
/// Every node denotes a [`TimeIntervalSequence`]: each instant is active,
/// inactive or unknown, and the node's extent says where it is known. `Not`,
/// `And`, `Or` and `Shift` call [`complement`], [`intersection`], [`union`] and
/// [`transform`], so they mean exactly what those mean, extent included, and an
/// empty `And` is active everywhere while an empty `Or` is active nowhere, both
/// observed everywhere. One difference: a `Shift` never fails. A bound at
/// either end of the timestamp range stands for "without end" and stays put,
/// as an extent's does, and any other bound saturates. An open span on the
/// default extent reaches the end of time, so a formula shifting it must not
/// overflow. The leaves are defined here.
///
/// - **`State`** is the member's timeline, extent included.
/// - **`Between`** reads two patterns, which may name one member or two. The
///   formula is observed over the instants both members are, and it is
///   evaluated on each run of that extent by itself, starting inactive. Points
///   outside the extent are ignored. In the run, the points matching either
///   pattern are walked in time order, ties in the order they were recorded,
///   and when the members differ the points of `start` come before those of
///   `end` at one instant. A point is read once: while inactive, a match of
///   `start` opens a span, and while active, a match of `end` closes it
///   there. A start while active and an end while inactive do nothing, so the
///   formula is total and a malformed stream is no error. A span that would
///   be empty, closed at its own start, lasts the one tick `[s, s + 1)`, and a
///   start at `i64::MAX`, which has no room for a tick, opens nothing. A span
///   still open at the end of the run stops where [`OpenEnd`] says.
/// - **`While`** holds the last sample: at `t` it is active when the last
///   point of the member at or before `t`, within the same run of its extent,
///   matches, and where several points share an instant the last recorded
///   one is the sample. Before the first point of a run the value is unknown,
///   so the formula is observed from there to the end of the run. With a
///   `timeout`, a sample at `t` holds on `[t, min(next sample, t + timeout))`
///   and the formula is unknown from `t + timeout` to the next sample, or to
///   the end of the run: a sensor that went silent says nothing about what
///   follows, so the instants after it are neither active nor inactive.
/// - **`Occurs`** is observed wherever the member is, and active on the
///   instant of each matching point: a point says nothing about the instants
///   between points.
///
/// The label of the timeline a formula evaluates to is the formula written
/// out.
///
/// # Step values
///
/// A value that holds until it changes (an occupancy, a counter) needs no
/// sequence shape of its own: it is a point sequence of its changes, read with
/// sample and hold, which is what `While` does, so "count above five" is
/// `while_matching`. [`while_within`](Formula::while_within) adds a timeout for
/// a sensor that may go silent: its last sample stops holding after that many
/// ticks and what follows is unknown, not a value. [`Events::Changes`] is the
/// canonical form of a step, the points with each repeat dropped, for a host
/// that stores or streams only the changes. A live stream system decides a
/// sample's hold once the frontier passes either its next sample or its
/// timeout, so `While` has no lag with or without one.
///
/// ```
/// use chronoloom::stream_system::StreamSystem;
/// use chronoloom::{Condition, Formula, Pattern, Processor};
/// use chronoloom::{TimePointEvent, TimePointSequence};
///
/// let mut room = StreamSystem::new();
/// room.insert_stream("count", TimePointSequence::from_events("count", vec![
///     TimePointEvent::new(0, 2_i64),
///     TimePointEvent::new(10, 9),
///     TimePointEvent::new(100, 1),
/// ]))?;
///
/// // Busy while the last count is above five, for at most 30 ticks after it.
/// let busy = Formula::while_within(Pattern::new("count", Condition::Greater(5_i64)), 30);
/// let busy = Processor::new("busy", busy).evaluate(&room)?;
/// let busy = busy.as_time_interval().unwrap();
///
/// assert_eq!(busy[0].bounds(), (10, 40));
/// // Past the timeout the sensor was silent: unknown, not inactive.
/// assert!(!busy.contains(50) && !busy.is_observed(50));
/// assert!(busy.is_observed(100));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Normal form
///
/// [`conjunctive_normal_form`] rewrites a formula as an `And` of `Or`s of
/// literals, a literal being a leaf, a `Shift` or their `Not`, which denotes
/// the same timeline, extent included. It pushes every `Not` inward and
/// distributes `Or` over `And`, which three-valued logic allows because it is
/// Kleene's: its "and" and "or" are distributive and `¬(a ∧ b)` is `¬a ∨ ¬b`.
/// A `Shift` is an atom, since a complement does not commute with it, but the
/// formula inside it is normalized too. This is one more way to write a state,
/// not a separate mechanism.
///
/// ```
/// use chronoloom::{Condition, Formula, Pattern};
///
/// let hot = Formula::while_matching(Pattern::new("temperature", Condition::Greater(30.0_f64)));
/// let humid = Formula::while_matching(Pattern::new("humidity", Condition::Greater(80.0_f64)));
/// let stuffy = !hot.clone().and(humid.clone());
///
/// assert_eq!(stuffy.to_string(), "¬(while(temperature > 30.0) ∧ while(humidity > 80.0))");
/// assert_eq!(
///     stuffy.conjunctive_normal_form().to_string(),
///     "(¬while(temperature > 30.0) ∨ ¬while(humidity > 80.0))",
/// );
/// ```
///
/// [`Condition::Within`]: super::Condition::Within
/// [`Events::Changes`]: super::Events::Changes
/// [`complement`]: TimeIntervalSequence::complement
/// [`conjunctive_normal_form`]: Formula::conjunctive_normal_form
/// [`intersection`]: TimeIntervalSequence::intersection
/// [`transform`]: TimeIntervalSequence::transform
/// [`union`]: TimeIntervalSequence::union
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Formula {
    /// A timeline member, as it is.
    State(Key),
    /// Active from a point matching `start` until the next one matching `end`.
    Between {
        /// The points that open a span.
        start: Pattern,
        /// The points that close it.
        end: Pattern,
        /// Where a span with no end stops.
        #[cfg_attr(feature = "serde", serde(default))]
        open: OpenEnd,
    },
    /// Active while the last sample of the pattern's member matches, and for
    /// at most `timeout` ticks after it if there is a timeout.
    While {
        /// The points that are samples.
        pattern: Pattern,
        /// How long a sample holds, if it does: unknown after that.
        #[cfg_attr(
            feature = "serde",
            serde(default, deserialize_with = "crate::serialisation::positive_timeout")
        )]
        timeout: Option<Timestamp>,
    },
    /// Active on the instant of each matching point.
    Occurs(Pattern),
    /// The observed instants where the formula is not active.
    Not(Box<Formula>),
    /// Active where every formula is.
    And(Vec<Formula>),
    /// Active where at least one formula is.
    Or(Vec<Formula>),
    /// The formula with every span `[s, e)` moved to `[s + alpha, e + beta)`.
    Shift {
        /// The formula to shift.
        formula: Box<Formula>,
        /// The shift of every lower bound.
        alpha: Timestamp,
        /// The shift of every upper bound.
        beta: Timestamp,
    },
}

impl Formula {
    /// Every key the formula reads, with repeats, in the order the tree is
    /// walked.
    pub(super) fn read_keys<'a>(&'a self, keys: &mut Vec<&'a Key>) {
        match self {
            Self::State(key) => keys.push(key),
            Self::Between { start, end, .. } => keys.extend([&start.key, &end.key]),
            Self::While { pattern, .. } | Self::Occurs(pattern) => keys.push(&pattern.key),
            Self::Not(inner) => inner.read_keys(keys),
            Self::And(formulas) | Self::Or(formulas) => {
                for formula in formulas {
                    formula.read_keys(keys);
                }
            }
            Self::Shift { formula, .. } => formula.read_keys(keys),
        }
    }

    /// The timeline member under `key`, as it is.
    #[must_use]
    pub fn state(key: impl Into<Key>) -> Self {
        Self::State(key.into())
    }

    /// Active from a point matching `start` until the next one matching `end`,
    /// with a span that has no end stopping at the end of the extent.
    #[must_use]
    pub const fn between(start: Pattern, end: Pattern) -> Self {
        Self::between_with(start, end, OpenEnd::Extent)
    }

    /// [`between`](Formula::between) with a say in where a span with no end
    /// stops.
    #[must_use]
    pub const fn between_with(start: Pattern, end: Pattern, open: OpenEnd) -> Self {
        Self::Between { start, end, open }
    }

    /// Active while the last sample of the pattern's member matches.
    #[must_use]
    pub const fn while_matching(pattern: Pattern) -> Self {
        Self::While {
            pattern,
            timeout: None,
        }
    }

    /// [`while_matching`](Formula::while_matching) where a sample stops holding
    /// `timeout` ticks after it was taken, and what follows, until the next
    /// sample, is unknown.
    ///
    /// # Panics
    ///
    /// If `timeout` is not positive: a sample that holds no time is no sample.
    /// Like [`hold`](Formula::hold), the argument is a programming error rather
    /// than data, so it is not a `Result`.
    #[must_use]
    pub fn while_within(pattern: Pattern, timeout: Timestamp) -> Self {
        assert!(
            timeout > 0,
            "a sample holds at least one tick, got {timeout}"
        );
        Self::While {
            pattern,
            timeout: Some(timeout),
        }
    }

    /// Active on the instant of each point the pattern matches.
    #[must_use]
    pub const fn occurs(pattern: Pattern) -> Self {
        Self::Occurs(pattern)
    }

    /// Active for `ticks` ticks from each point the pattern matches: a point at
    /// `t` makes `[t, t + ticks)` active, and holds that overlap or touch merge.
    ///
    /// This is `Shift { Occurs(pattern), 0, ticks - 1 }` and no variant of its
    /// own, so it is three-valued like any shift: an instant is known when it
    /// is active, or when the `ticks` instants up to it were all observed. It
    /// lags the member by nothing when run online, and `hold(pattern, 1)` is
    /// `Occurs`.
    /// A hold that would reach past the last timestamp saturates there.
    ///
    /// # Panics
    ///
    /// If `ticks` is not positive: a hold of no duration is no span, and a
    /// negative one would be a shift back in time, which [`shift`](Formula::shift)
    /// says better. Like the range of a point sequence, the argument is a
    /// programming error rather than data, so it is not a `Result`.
    #[must_use]
    pub fn hold(pattern: Pattern, ticks: Timestamp) -> Self {
        assert!(ticks > 0, "a hold lasts at least one tick, got {ticks}");
        Self::occurs(pattern).shift(0, ticks - 1)
    }

    /// Active where both this and `other` are. Chains read flat: `a.and(b)
    /// .and(c)` is one `And` of three.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match self {
            Self::And(mut formulas) => {
                formulas.push(other);
                Self::And(formulas)
            }
            formula => Self::And(vec![formula, other]),
        }
    }

    /// Active where either this or `other` is. Chains read flat: `a.or(b)
    /// .or(c)` is one `Or` of three.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        match self {
            Self::Or(mut formulas) => {
                formulas.push(other);
                Self::Or(formulas)
            }
            formula => Self::Or(vec![formula, other]),
        }
    }

    /// This formula with every span `[s, e)` moved to `[s + alpha, e + beta)`.
    #[must_use]
    pub fn shift(self, alpha: Timestamp, beta: Timestamp) -> Self {
        Self::Shift {
            formula: Box::new(self),
            alpha,
            beta,
        }
    }

    /// An `And` of `Or`s of literals that denotes the same timeline as this
    /// formula, extent included.
    ///
    /// A literal is a leaf (`State`, `Between`, `While`, `Occurs`) or a
    /// `Shift`, plain or under one `Not`. The result is always an `And`, of one
    /// `Or` per clause, even for a single literal, so its shape can be relied
    /// on; a clause or literal that repeats is kept once.
    #[must_use]
    pub fn conjunctive_normal_form(&self) -> Self {
        let mut clauses: Vec<Vec<Self>> = Vec::new();
        for clause in negation_normal_form(self, false).clauses() {
            if !clauses.contains(&clause) {
                clauses.push(clause);
            }
        }

        Self::And(clauses.into_iter().map(Self::Or).collect())
    }
}

impl Not for Formula {
    type Output = Self;

    /// The observed instants where the formula is not active.
    fn not(self) -> Self {
        Self::Not(Box::new(self))
    }
}

/// A formula with every `Not` pushed down to a literal.
enum Normal {
    Literal(Formula),
    And(Vec<Normal>),
    Or(Vec<Normal>),
}

/// `formula`, or its negation, with every `Not` pushed down to a literal.
fn negation_normal_form(formula: &Formula, negated: bool) -> Normal {
    let all = |formulas: &[Formula]| {
        formulas
            .iter()
            .map(|formula| negation_normal_form(formula, negated))
            .collect()
    };

    match formula {
        Formula::Not(inner) => negation_normal_form(inner, !negated),
        Formula::And(formulas) if negated => Normal::Or(all(formulas)),
        Formula::And(formulas) => Normal::And(all(formulas)),
        Formula::Or(formulas) if negated => Normal::And(all(formulas)),
        Formula::Or(formulas) => Normal::Or(all(formulas)),
        atom => {
            let atom = match atom {
                // Inside a shift the formula is a timeline of its own, which
                // can be normalized without touching the shift.
                Formula::Shift {
                    formula,
                    alpha,
                    beta,
                } => Formula::Shift {
                    formula: Box::new(formula.conjunctive_normal_form()),
                    alpha: *alpha,
                    beta: *beta,
                },
                other => other.clone(),
            };
            Normal::Literal(if negated { !atom } else { atom })
        }
    }
}

impl Normal {
    /// The clauses of the conjunctive normal form: a conjunction of
    /// disjunctions of literals, written as lists.
    fn clauses(self) -> Vec<Vec<Formula>> {
        match self {
            Self::Literal(literal) => vec![vec![literal]],
            Self::And(parts) => parts.into_iter().flat_map(Self::clauses).collect(),
            Self::Or(parts) => {
                // One empty clause, which is false, and each part either
                // extends every clause or, if it is true, absorbs them all.
                let mut product: Vec<Vec<Formula>> = vec![Vec::new()];
                for part in parts {
                    let alternatives = part.clauses();
                    let mut next = Vec::with_capacity(product.len() * alternatives.len());
                    for clause in &product {
                        for alternative in &alternatives {
                            let mut joined = clause.clone();
                            for literal in alternative {
                                if !joined.contains(literal) {
                                    joined.push(literal.clone());
                                }
                            }
                            next.push(joined);
                        }
                    }
                    product = next;
                }
                product
            }
        }
    }
}

impl fmt::Display for Formula {
    /// The formula in the notation of the documentation, with parentheses where
    /// leaving them out would change what it means.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(key) => write!(f, "{key}"),
            Self::Between {
                start,
                end,
                open: OpenEnd::Extent,
            } => write!(f, "between({start}, {end})"),
            Self::Between {
                start,
                end,
                open: OpenEnd::Head,
            } => write!(f, "between({start}, {end}, open until head)"),
            Self::While {
                pattern,
                timeout: None,
            } => write!(f, "while({pattern})"),
            Self::While {
                pattern,
                timeout: Some(timeout),
            } => write!(f, "while({pattern}, within {timeout})"),
            Self::Occurs(pattern) => write!(f, "occurs({pattern})"),
            Self::Not(formula) => {
                f.write_str("¬")?;
                write_operand(formula, matches!(**formula, Self::And(_) | Self::Or(_)), f)
            }
            Self::And(formulas) => write_all(formulas, "∧", "⊤", f),
            Self::Or(formulas) => write_all(formulas, "∨", "⊥", f),
            Self::Shift {
                formula,
                alpha,
                beta,
            } => {
                write_operand(
                    formula,
                    matches!(**formula, Self::And(_) | Self::Or(_) | Self::Not(_)),
                    f,
                )?;
                write!(f, "[{alpha}, {beta}]")
            }
        }
    }
}

/// Write `formula`, in parentheses when `grouped`.
fn write_operand(formula: &Formula, grouped: bool, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if grouped {
        write!(f, "({formula})")
    } else {
        write!(f, "{formula}")
    }
}

/// Write `formulas` joined by `symbol`, or `empty` when there are none.
fn write_all(
    formulas: &[Formula],
    symbol: &str,
    empty: &str,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    if formulas.is_empty() {
        return f.write_str(empty);
    }

    for (index, formula) in formulas.iter().enumerate() {
        if index > 0 {
            write!(f, " {symbol} ")?;
        }
        write_operand(
            formula,
            matches!(formula, Formula::And(_) | Formula::Or(_)),
            f,
        )?;
    }

    Ok(())
}
