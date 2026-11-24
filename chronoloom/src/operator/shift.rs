//! `Shift`: the timeline of the inner node, moved and widened.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::sequence_system::{Sequence, SequenceSystem};
use crate::sequences::TimeIntervalSequence;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::plan::Node;

#[derive(Debug, Clone)]
pub(super) struct Shift {
    inner: Box<Node>,
    alpha: Timestamp,
    beta: Timestamp,
}

impl Shift {
    pub(super) fn new(inner: Node, alpha: Timestamp, beta: Timestamp) -> Self {
        Self {
            inner: Box::new(inner),
            alpha,
            beta,
        }
    }

    pub(super) fn online(&self) -> OnlineNode {
        OnlineNode::Shift(OnlineShift {
            inner: Box::new(self.inner.online()),
            alpha: self.alpha,
            beta: self.beta,
            frontier: Timestamp::MIN,
            held: None,
        })
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        Ok(self
            .inner
            .batch(inputs)?
            .transform_unbounded(self.alpha, self.beta))
    }
}

/// The running state of [`Shift`]: a delay buffer over the timeline of the
/// inner node.
///
/// An instant `t` of the shifted timeline reads the instants of the inner one
/// from `t - max(alpha, beta)` to `t - min(alpha, beta)`, so it is final once
/// the inner node has reached past that. The output therefore stops
/// `min(alpha, beta)` after the frontier of the inner node when that is
/// negative (a look-ahead shift lags), and at the frontier itself otherwise,
/// since an output never runs ahead of its inputs. What is held is the inner
/// timeline from `max(alpha, beta)` before the output frontier on, which is all
/// a later instant can read.
#[derive(Debug, Clone)]
pub(crate) struct OnlineShift {
    inner: Box<OnlineNode>,
    alpha: Timestamp,
    beta: Timestamp,
    frontier: Timestamp,
    held: Option<Chunk>,
}

impl OnlineShift {
    /// # Panics
    ///
    /// If the inner node gives a chunk that is not a timeline, which no online
    /// node does.
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let chunk = self.inner.step(inputs, to)?;
        let held = match self.held.take() {
            Some(earlier) => earlier.join(chunk),
            None => chunk,
        };

        let reached = held.to();
        let limit = if reached == Timestamp::MAX {
            reached
        } else {
            reached
                .saturating_add(self.alpha.min(self.beta))
                .min(reached)
        }
        .min(to)
        .max(self.frontier);

        let frontier = self.frontier;
        let shifted = held
            .timeline()
            .expect("online nodes give timelines")
            .transform_unbounded(self.alpha, self.beta);
        let output = Chunk::of(&Sequence::TimeInterval(shifted), frontier, limit);

        // Saturation at the start of time can push this past what is held.
        let keep_from = limit.saturating_sub(self.alpha.max(self.beta)).min(reached);
        self.held = Some(held.slice(keep_from, reached));
        self.frontier = limit;
        Ok(output)
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}
