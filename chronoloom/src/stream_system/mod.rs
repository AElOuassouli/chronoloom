//! One system of streams and the processors that derive new streams from them.
//!
//! [`StreamSystem`] is the main abstraction; see its documentation.

mod batch;
mod errors;
mod mode;
mod pipeline;
mod report;
mod shared;
mod system;
pub(crate) mod update;

pub use errors::UpdateError;
pub use mode::{Batch, Mode};
pub(crate) use pipeline::writes;
pub use report::Report;
pub use system::StreamSystem;
pub use update::Update;
