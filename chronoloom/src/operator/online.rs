//! The online form of a node: it steps over the newly final range of its inputs.
//!
//! `step(inputs, to)` reads the new chunk of every member the node reads and
//! returns the chunk of its own output from its frontier up to the frontier it
//! reaches, which is at most `to` and never beyond what its inputs have
//! reached. Joined end to end from the first instant, the chunks equal what
//! `batch` gives over the whole input.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;

use super::and::OnlineAnd;
use super::between::OnlineBetween;
use super::chunk::{Chunk, Inputs};
use super::not::OnlineNot;
use super::occurs::OnlineOccurs;
use super::or::OnlineOr;
use super::r#while::OnlineWhile;
use super::shift::OnlineShift;
use super::state::OnlineState;

/// The running state of one node whose result is a timeline.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub(crate) enum OnlineNode {
    State(OnlineState),
    Occurs(OnlineOccurs),
    Not(OnlineNot),
    And(OnlineAnd),
    Or(OnlineOr),
    Between(OnlineBetween),
    While(OnlineWhile),
    Shift(OnlineShift),
}

impl OnlineNode {
    /// The output over `[frontier, new frontier)`, the new frontier being at
    /// most `to`. Chunks of `inputs` start at the frontier of the member.
    ///
    /// # Errors
    ///
    /// If a member the node reads is missing from `inputs`, or is of another
    /// shape or value type than the node reads.
    pub(crate) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        match self {
            Self::State(node) => node.step(inputs, to),
            Self::Occurs(node) => node.step(inputs, to),
            Self::Not(node) => node.step(inputs, to),
            Self::And(node) => node.step(inputs, to),
            Self::Or(node) => node.step(inputs, to),
            Self::Between(node) => node.step(inputs, to),
            Self::While(node) => node.step(inputs, to),
            Self::Shift(node) => node.step(inputs, to),
        }
    }

    /// How far the output is final: the end of the last chunk returned, and
    /// never beyond the frontier of an input.
    pub(crate) fn frontier(&self) -> Timestamp {
        match self {
            Self::State(node) => node.frontier(),
            Self::Occurs(node) => node.frontier(),
            Self::Not(node) => node.frontier(),
            Self::And(node) => node.frontier(),
            Self::Or(node) => node.frontier(),
            Self::Between(node) => node.frontier(),
            Self::While(node) => node.frontier(),
            Self::Shift(node) => node.frontier(),
        }
    }
}

#[cfg(feature = "serde")]
impl OnlineNode {
    /// Whether the node is of the same kind as `fresh`, compiled from the
    /// processor, with the same fixed parts all the way down: the running state
    /// is what may differ.
    pub(crate) fn same_plan(&self, fresh: &Self) -> bool {
        match (self, fresh) {
            (Self::State(saved), Self::State(fresh)) => saved.same_plan(fresh),
            (Self::Occurs(saved), Self::Occurs(fresh)) => saved.same_plan(fresh),
            (Self::Not(saved), Self::Not(fresh)) => saved.same_plan(fresh),
            (Self::And(saved), Self::And(fresh)) => saved.same_plan(fresh),
            (Self::Or(saved), Self::Or(fresh)) => saved.same_plan(fresh),
            (Self::Between(saved), Self::Between(fresh)) => saved.same_plan(fresh),
            (Self::While(saved), Self::While(fresh)) => saved.same_plan(fresh),
            (Self::Shift(saved), Self::Shift(fresh)) => saved.same_plan(fresh),
            _ => false,
        }
    }
}
