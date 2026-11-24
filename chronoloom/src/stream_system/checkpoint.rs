//! What a checkpoint of a live stream system holds, as the parts it is
//! restored from.

use crate::primitives::Timestamp;
use crate::processor::Processor;
use crate::sequence_system::{Key, Metadata, Sequence};
use crate::values::Value;

use super::mode::LiveNode;

/// A stream of a checkpoint.
pub(crate) struct CheckpointStream {
    pub(crate) key: Key,
    pub(crate) sequence: Sequence,
    pub(crate) metadata: Metadata,
    pub(crate) frontier: Timestamp,
}

/// A derived stream of a checkpoint.
pub(crate) struct CheckpointDerived {
    pub(crate) key: Key,
    pub(crate) sequence: Sequence,
    pub(crate) metadata: Metadata,
    pub(crate) entity: Option<Value>,
}

/// The parts of a checkpoint.
pub(crate) struct Checkpoint {
    pub(crate) from: Timestamp,
    pub(crate) metadata: Metadata,
    pub(crate) streams: Vec<CheckpointStream>,
    pub(crate) processors: Vec<Processor>,
    pub(crate) nodes: Vec<LiveNode>,
    pub(crate) derived: Vec<CheckpointDerived>,
}
