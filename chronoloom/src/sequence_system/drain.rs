//! Draining: handing over the final part of the live members.

use std::collections::BTreeMap;

use crate::primitives::Timestamp;
use crate::sequences::Extent;

use super::erased::AnyPointSequence;
use super::{Key, Member, Sequence, SequenceSystem};

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
    /// Take the part of every member before the instant `cuts` gives for its
    /// key out of the system, and return it as a system of batch members that
    /// keep their metadata, lineage, entity and schema. A member without a cut
    /// is left alone and absent from the result.
    ///
    /// As [`drain_finalised`](SequenceSystem::drain_finalised) does for the
    /// frontier of a live member, a span reaching the cut stays whole.
    pub(crate) fn drain_before(&mut self, cuts: &BTreeMap<Key, Timestamp>) -> Self {
        let mut members = BTreeMap::new();

        for (key, member) in &mut self.members {
            let Some(&cut) = cuts.get(key) else {
                continue;
            };
            let drained = Member {
                sequence: member.sequence.split_finalised(cut),
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

    /// Narrow the member for `key` to `extent`, as
    /// [`observe`](SequenceSystem::observe) does for all of them. An unknown
    /// key is left alone.
    pub(crate) fn observe_member(&mut self, key: &str, extent: &Extent) {
        if let Some(member) = self.members.get_mut(key) {
            match &mut member.sequence {
                Sequence::TimePoint(points) => points.as_erased_mut().observe(extent),
                Sequence::TimeInterval(timeline) => timeline.observe(extent),
            }
        }
    }

    /// Observe the member for `key` from `from` on, whatever it observed
    /// before: what a stream that is going live has beyond its cut. An unknown
    /// key is left alone.
    pub(crate) fn observe_from(&mut self, key: &str, from: Timestamp) {
        if let Some(member) = self.members.get_mut(key) {
            member
                .observed_mut()
                .grow(Extent::of_range(&(from..)).as_slice());
        }
    }
}
