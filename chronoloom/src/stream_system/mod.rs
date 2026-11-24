//! One system of streams and the processors that derive new streams from them.
//!
//! [`StreamSystem`] is the main abstraction; see its documentation.

mod batch;
#[cfg(feature = "serde")]
pub(crate) mod checkpoint;
mod errors;
mod live;
mod mode;
mod pipeline;
mod report;
mod shared;
mod system;
pub(crate) mod update;

pub use crate::operator::Chunk;
pub use crate::store::{
    AnyPointSequence, AnyValue, Key, Metadata, PointValue, RecordError, Sequence,
};
pub use errors::UpdateError;
#[cfg(feature = "serde")]
pub(crate) use mode::LiveNode;
pub use mode::{Batch, Live, Mode};
pub(crate) use pipeline::writes;
pub use report::{LiveReport, Report};
pub use system::StreamSystem;
pub use update::Update;
