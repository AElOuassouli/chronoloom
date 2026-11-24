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
pub(super) struct Combination {
    parts: Vec<OnlineNode>,
    /// What each part produced beyond the frontier, from the frontier on.
    pending: Vec<Option<Chunk>>,
    frontier: Timestamp,
    /// The result with no part at all, which is also the neutral element.
    identity: TimeIntervalSequence,
    combine: fn(&TimeIntervalSequence, &TimeIntervalSequence) -> TimeIntervalSequence,
}

impl Combination {
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
