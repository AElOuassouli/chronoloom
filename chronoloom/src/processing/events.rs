//! Formulas whose result is a point sequence.

use core::fmt;

use crate::operator::PointsPlan;
use crate::sequence_system::{Key, Sequence, SequenceSystem};

use super::condition::Pattern;
use super::errors::ProcessError;
use super::formula::Formula;
use super::kind::Kind;

/// What a point output is: formulas whose result is a point sequence rather
/// than a timeline.
///
/// - `Edges(formula)` reads a state as events: a `TimePointSequence<Edge>`
///   with an [`Edge::Start`] at the first instant of each span and an
///   [`Edge::End`] at the first instant after it, in the extent of the
///   formula. The end of a span that reaches the end of its extent sits on that
///   exclusive end, outside the extent, and is kept: the edge is the boundary,
///   not an observation of the instant. Reading the result back with
///   `Between(Edge = Start, Edge = End)` rebuilds the state.
/// - `Matching(pattern)` is the points of the pattern's member whose value
///   matches, values kept and extent unchanged, so a filter over any value
///   type is available without a conversion.
/// - `Project { key, path, into }` reads one field of the record values of the
///   member under `key`, the [`Value`] at the dotted `path` (see [`Value::get`]),
///   out into a point sequence of its own: the type of the points is the one
///   `into` stands for (see [`Kind`]), and the timestamps and the extent are the
///   member's. The values are copied, so the result is independent of the
///   member. An event whose record has no such field is skipped, which is
///   normal for an `Optional` field, and a `Null` field is skipped too, except
///   when projecting into `Kind::Value`, which keeps it. A field of another kind
///   than `into` (a float into `Integer`, text into `Bool`; an integer widens
///   into `Float`) is [`ProcessError::ProjectionMismatch`], and the member must
///   hold `Value` points ([`ProcessError::ValueTypeMismatch`] otherwise). A live
///   processor supports it like `Matching`: it emits the projected points once
///   final, as [`Emitted::Point`] of the type `into` stands for, and what it
///   emitted equals the batch result.
///
/// - `Changes { key }` is the points of the member under `key` with the repeats
///   dropped: a point is kept when its value differs (by `PartialEq`) from the
///   sample before it, and the first sample of each run of the extent is kept,
///   since what came before a run is unknown. Where several points share an
///   instant the last recorded is the sample, as for [`Formula::While`], so the
///   result has at most one point per instant. It is the canonical form of a
///   step value, a point sequence of changes read with sample and hold:
///   applying it twice changes nothing, and a `While` over the changes means
///   what it means over the original. The member may hold any value type, and
///   the points and extent are the member's own. A live processor supports it
///   like `Matching`: it emits each point of the result, erased as an
///   [`Emitted::Point`], once it is final, which is when the frontier of the
///   member passes it. A member that is only read by a `Changes` has no value
///   type until its first event arrives, and takes that of the first
///   `on_point`.
///
/// The label of the sequence an `Events` evaluates to is the formula written
/// out: `edges(up)`, `matching(door = "open")`, `changes(count)`; for a projection it is the
/// field written as `key.path`, or `key` for the empty path.
///
/// ```
/// use chronoloom::{
///     Events, Kind, SequenceSystem, TimePointEvent, TimePointSequence, Value,
/// };
///
/// let mut system = SequenceSystem::new();
/// system.insert("sensor", TimePointSequence::from_events("sensor", vec![
///     TimePointEvent::new(10, Value::record([("agent", Value::record([("id", Value::from(7))]))])),
///     TimePointEvent::new(20, Value::record([("note", Value::from("idle"))])),
///     TimePointEvent::new(30, Value::record([("agent", Value::record([("id", Value::from(9))]))])),
/// ]));
///
/// let agents = Events::project("sensor", "agent.id", Kind::Integer).evaluate(&system)?;
///
/// // The event without an agent is skipped.
/// let agents = agents.as_time_point::<i64>().unwrap();
/// let seen: Vec<(i64, i64)> = agents.iter().map(|e| (e.timestamp(), *e.value())).collect();
/// assert_eq!(seen, [(10, 7), (30, 9)]);
/// assert_eq!(agents.label(), "sensor.agent.id");
/// # Ok::<(), chronoloom::ProcessError>(())
/// ```
///
/// [`Edge::End`]: super::Edge::End
/// [`Edge::Start`]: super::Edge::Start
/// [`Emitted::Point`]: super::Emitted::Point
/// [`ProcessError::ProjectionMismatch`]: super::ProcessError::ProjectionMismatch
/// [`ProcessError::ValueTypeMismatch`]: super::ProcessError::ValueTypeMismatch
/// [`Value`]: crate::Value
/// [`Value::get`]: crate::Value::get
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Events {
    /// The starts and ends of the spans of a state.
    Edges(Formula),
    /// The points of a member whose value matches.
    Matching(Pattern),
    /// The points of a member, each kept only if its value differs from the
    /// sample before it.
    Changes {
        /// The member of points to read.
        key: Key,
    },
    /// One field of the record values of a member, as points of a type.
    Project {
        /// The member of `Value` points to read.
        key: Key,
        /// The dotted path of the field.
        path: String,
        /// The type of the points of the result.
        into: Kind,
    },
}

impl Events {
    /// The field at `path` of the records of the member under `key`, read
    /// into points of the type `into` stands for.
    #[must_use]
    pub fn project(key: impl Into<Key>, path: impl Into<String>, into: Kind) -> Self {
        Self::Project {
            key: key.into(),
            path: path.into(),
            into,
        }
    }

    /// The points of the member under `key` with the repeats dropped.
    #[must_use]
    pub fn changes(key: impl Into<Key>) -> Self {
        Self::Changes { key: key.into() }
    }

    /// Every key the events read, with repeats.
    pub(super) fn read_keys<'a>(&'a self, keys: &mut Vec<&'a Key>) {
        match self {
            Self::Edges(formula) => formula.read_keys(keys),
            Self::Matching(pattern) => keys.push(&pattern.key),
            Self::Project { key, .. } | Self::Changes { key } => keys.push(key),
        }
    }

    /// The point sequence the events denote over the members of `system`.
    ///
    /// # Errors
    ///
    /// As [`Formula::evaluate`], [`ProcessError::NotPoints`] or
    /// [`ProcessError::UnknownKey`] for a `Changes` of a member that is not a
    /// point sequence, [`ProcessError::NotPoints`] or
    /// [`ProcessError::ValueTypeMismatch`] for a pattern or projection the
    /// member cannot satisfy, and [`ProcessError::ProjectionMismatch`] for a
    /// field that is not of the kind projected.
    pub fn evaluate(&self, system: &SequenceSystem) -> Result<Sequence, ProcessError> {
        PointsPlan::compile(self).batch(system).map(Sequence::from)
    }
}

impl fmt::Display for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edges(formula) => write!(f, "edges({formula})"),
            Self::Matching(pattern) => write!(f, "matching({pattern})"),
            Self::Changes { key } => write!(f, "changes({key})"),
            Self::Project { key, path, .. } if path.is_empty() => write!(f, "{key}"),
            Self::Project { key, path, .. } => write!(f, "{key}.{path}"),
        }
    }
}
