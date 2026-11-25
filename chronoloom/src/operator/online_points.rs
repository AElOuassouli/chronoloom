//! The online form of the root of events: it steps over the newly final range
//! of its inputs, like an [`OnlineNode`](super::online::OnlineNode), but its
//! chunks hold points.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;

use super::changes::OnlineChanges;
use super::chunk::{Chunk, Inputs};
use super::edges::OnlineEdges;
use super::matching::OnlineMatching;
use super::project::OnlineProject;

/// The running state of events.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub(crate) enum OnlinePoints {
    Edges(OnlineEdges),
    Matching(OnlineMatching),
    Changes(OnlineChanges),
    Project(OnlineProject),
}

impl OnlinePoints {
    /// The points over `[frontier, new frontier)`, the new frontier being at
    /// most `to`. Chunks of `inputs` start at the frontier of the member.
    ///
    /// # Errors
    ///
    /// If a member the events read is missing from `inputs`, or is of another
    /// shape or value type than they read.
    pub(crate) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        match self {
            Self::Edges(node) => node.step(inputs, to),
            Self::Matching(node) => node.step(inputs, to),
            Self::Changes(node) => node.step(inputs, to),
            Self::Project(node) => node.step(inputs, to),
        }
    }

    /// How far the output is final: the end of the last chunk returned, and
    /// never beyond the frontier of an input.
    pub(crate) fn frontier(&self) -> Timestamp {
        match self {
            Self::Edges(node) => node.frontier(),
            Self::Matching(node) => node.frontier(),
            Self::Changes(node) => node.frontier(),
            Self::Project(node) => node.frontier(),
        }
    }
}

#[cfg(feature = "serde")]
impl OnlinePoints {
    /// Whether the node is of the same kind as `fresh`, compiled from the
    /// processor, with the same fixed parts.
    pub(crate) fn same_plan(&self, fresh: &Self) -> bool {
        match (self, fresh) {
            (Self::Edges(saved), Self::Edges(fresh)) => saved.same_plan(fresh),
            (Self::Matching(saved), Self::Matching(fresh)) => saved.same_plan(fresh),
            (Self::Changes(saved), Self::Changes(fresh)) => saved.same_plan(fresh),
            (Self::Project(saved), Self::Project(fresh)) => saved.same_plan(fresh),
            _ => false,
        }
    }
}
