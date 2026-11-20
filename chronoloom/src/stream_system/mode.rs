//! The modes a stream system can be in.

mod sealed {
    pub trait Sealed {}
}

/// The mode of a [`StreamSystem`]: [`Batch`] today. Sealed: the modes are the
/// crate's own.
///
/// [`StreamSystem`]: super::StreamSystem
pub trait Mode: sealed::Sealed + Clone + core::fmt::Debug + PartialEq {}

/// The batch mode of a [`StreamSystem`]: a dataset held whole, whose derived
/// streams are evaluated again whenever what they read changes.
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Batch;

impl sealed::Sealed for Batch {}
impl Mode for Batch {}
