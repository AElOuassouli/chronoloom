//! Cutting a system to a range of time.

use std::ops::RangeBounds;

use crate::primitives::Timestamp;

use super::erased::AnyPointSequence;
use super::{Member, Sequence, Store};

impl Sequence {
    /// The part of the sequence inside `range`, as [`slice`] does for either
    /// shape.
    ///
    /// [`slice`]: crate::sequences::TimeIntervalSequence::slice
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<Timestamp>) -> Self {
        match self {
            Self::TimePoint(points) => Self::TimePoint(AnyPointSequence(
                points
                    .0
                    .slice_bounds((range.start_bound().cloned(), range.end_bound().cloned())),
            )),
            Self::TimeInterval(timeline) => Self::TimeInterval(timeline.slice(range)),
        }
    }
}

impl Store {
    /// Every member cut to `range`, each keeping its key, label and metadata,
    /// and the system keeping its own metadata.
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<Timestamp>) -> Self {
        let bounds = (range.start_bound().cloned(), range.end_bound().cloned());

        Self {
            members: self
                .members
                .iter()
                .map(|(key, member)| {
                    let member = Member {
                        sequence: member.sequence.slice(bounds),
                        metadata: member.metadata.clone(),
                        derived: member.derived,
                        entity: member.entity.clone(),
                        schema: member.schema.clone(),
                    };
                    (key.clone(), member)
                })
                .collect(),
            metadata: self.metadata.clone(),
        }
    }
}
