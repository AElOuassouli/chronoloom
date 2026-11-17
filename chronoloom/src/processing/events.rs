//! Formulas whose result is a point sequence.

use core::fmt;

use crate::sequence_system::{Sequence, SequenceSystem};

use super::condition::Pattern;
use super::errors::ProcessError;
use super::evaluate;
use super::formula::Formula;

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
///
/// The label of the sequence an `Events` evaluates to is the formula written
/// out: `edges(up)`, `matching(door = "open")`.
///
/// [`Edge::End`]: super::Edge::End
/// [`Edge::Start`]: super::Edge::Start
#[derive(Debug, Clone, PartialEq)]
pub enum Events {
    /// The starts and ends of the spans of a state.
    Edges(Formula),
    /// The points of a member whose value matches.
    Matching(Pattern),
}

impl Events {
    /// The point sequence the events denote over the members of `system`.
    ///
    /// # Errors
    ///
    /// As [`Formula::evaluate`], and [`ProcessError::NotPoints`] or
    /// [`ProcessError::ValueTypeMismatch`] for a `Matching` pattern the member
    /// cannot satisfy.
    pub fn evaluate(&self, system: &SequenceSystem) -> Result<Sequence, ProcessError> {
        evaluate::evaluate_events(self, system)
    }
}

impl fmt::Display for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edges(formula) => write!(f, "edges({formula})"),
            Self::Matching(pattern) => write!(f, "matching({pattern})"),
        }
    }
}
