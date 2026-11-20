//! Draining: handing over the final part of the live members.

use std::collections::BTreeMap;

use crate::primitives::Timestamp;

use super::erased::AnyPointSequence;
use super::{Member, Sequence, SequenceSystem};

impl Sequence {
    /// Take out the part final at `frontier`, leaving the rest in place.
    fn split_finalised(&mut self, frontier: Timestamp) -> Self {
        match self {
            Self::TimePoint(points) => {
                Self::TimePoint(AnyPointSequence(points.0.split_before(frontier)))
            }
            Self::TimeInterval(timeline) => Self::TimeInterval(timeline.split_before(frontier)),
        }
    }
}

impl SequenceSystem {
    /// Take the final part of every live member out of the system, each by its
    /// own frontier, and return it as a system of batch members.
    ///
    /// Batch members are left alone and absent from the result. See
    /// [`SequenceSystem`] for what is drained and what stays.
    pub fn drain_finalised(&mut self) -> Self {
        let mut members = BTreeMap::new();

        for (key, member) in &mut self.members {
            let Some(frontier) = member.frontier else {
                continue;
            };
            let drained = Member {
                sequence: member.sequence.split_finalised(frontier),
                metadata: member.metadata.clone(),
                frontier: None,
                lineage: member.lineage.clone(),
                entity: member.entity.clone(),
                schema: member.schema.clone(),
            };
            members.insert(key.clone(), drained);
        }

        Self {
            members,
            metadata: self.metadata.clone(),
        }
    }
}

impl SequenceSystem {
    /// Discard the part of the live member for `key` before `cut`, as draining
    /// does but at an instant of the caller's choosing, never past the
    /// member's frontier. A batch member or an unknown key is left alone.
    pub(crate) fn discard_before(&mut self, key: &str, cut: Timestamp) {
        if let Some(member) = self.members.get_mut(key) {
            if let Some(frontier) = member.frontier {
                drop(member.sequence.split_finalised(cut.min(frontier)));
            }
        }
    }
}
