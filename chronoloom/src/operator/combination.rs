//! The online state shared by the nodes that combine several timelines.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::Sequence;
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;

/// Parts whose chunks are folded with one operation, as far as every part has
/// reached.
///
/// A part that is ahead of the others waits in `pending` until the slowest
/// catches up, so the frontier is the minimum of the parts'.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Combination {
    parts: Vec<OnlineNode>,
    /// What each part produced beyond the frontier, from the frontier on.
    pending: Vec<Option<Chunk>>,
    frontier: Timestamp,
    /// The result with no part at all, which is also the neutral element.
    identity: TimeIntervalSequence,
    /// Not saved: the node that owns the combination sets it back (see
    /// `OnlineAnd` and `OnlineOr`).
    #[cfg_attr(feature = "serde", serde(skip, default = "unset"))]
    combine: fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence,
}

#[cfg(feature = "serde")]
fn unset() -> fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence {
    |left, _| left.clone()
}

impl Combination {
    /// The combination with the operation of the node that owns it, as loaded.
    #[cfg(feature = "serde")]
    pub(super) fn with(
        mut self,
        combine: fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence,
    ) -> Self {
        self.combine = combine;
        self
    }

    pub(super) fn new(
        parts: Vec<OnlineNode>,
        identity: TimeIntervalSequence,
        combine: fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence,
    ) -> Self {
        Self {
            pending: vec![None; parts.len()],
            parts,
            frontier: Timestamp::MIN,
            identity,
            combine,
        }
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }

    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        for (part, pending) in self.parts.iter_mut().zip(&mut self.pending) {
            let chunk = part.step(inputs, to)?;
            *pending = Some(match pending.take() {
                Some(held) => held.join(chunk),
                None => chunk,
            });
        }

        let reached = self
            .pending
            .iter()
            .flatten()
            .map(Chunk::to)
            .min()
            .unwrap_or_else(|| to.max(self.frontier));

        let mut timeline = self.identity.slice(self.frontier..reached);
        for pending in &mut self.pending {
            if let Some(held) = pending.take() {
                let part = held.slice(self.frontier, reached);
                if let Some(part) = part.timeline() {
                    timeline = (self.combine)(&timeline, part);
                }
                *pending = Some(held.slice(reached, held.to()));
            }
        }

        let chunk = Chunk::new(self.frontier, reached, Sequence::TimeInterval(timeline));
        self.frontier = reached;
        Ok(chunk)
    }
}

#[cfg(feature = "serde")]
impl Combination {
    /// Whether the parts and the neutral element equal those of `fresh`,
    /// compiled from the processor, and what each part holds back is one slot
    /// per part.
    pub(super) fn same_plan(&self, fresh: &Self) -> bool {
        self.parts.len() == fresh.parts.len()
            && self.pending.len() == self.parts.len()
            && self.identity == fresh.identity
            && self
                .parts
                .iter()
                .zip(&fresh.parts)
                .all(|(saved, fresh)| saved.same_plan(fresh))
    }
}
