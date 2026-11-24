//! Assembling a system from its parts, for reading one back from a document.

use crate::primitives::Timestamp;
use crate::processing::Recipe;
use crate::values::{Schema, Value};

use super::{Key, Member, Metadata, Sequence, SequenceSystem};

/// Everything a system files under one key, as a document stores it.
pub(crate) struct MemberParts {
    pub(crate) key: Key,
    pub(crate) sequence: Sequence,
    pub(crate) metadata: Metadata,
    pub(crate) frontier: Option<Timestamp>,
    pub(crate) lineage: Option<Recipe>,
    pub(crate) entity: Option<Value>,
    pub(crate) schema: Option<Schema>,
}

impl SequenceSystem {
    /// A system holding exactly these members, with no check of its own: the
    /// caller has validated them. Where two share a key, the later one is kept.
    pub(crate) fn from_parts(
        metadata: Metadata,
        members: impl IntoIterator<Item = MemberParts>,
    ) -> Self {
        Self {
            members: members
                .into_iter()
                .map(|parts| {
                    let member = Member {
                        sequence: parts.sequence,
                        metadata: parts.metadata,
                        frontier: parts.frontier,
                        lineage: parts.lineage,
                        entity: parts.entity,
                        schema: parts.schema,
                    };
                    (parts.key, member)
                })
                .collect(),
            metadata,
        }
    }
}
