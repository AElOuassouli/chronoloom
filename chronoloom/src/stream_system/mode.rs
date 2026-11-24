//! The modes a stream system can be in.

use std::collections::BTreeMap;

use crate::operator::OnlineRecipe;
use crate::primitives::Timestamp;
use crate::sequence_system::Key;

mod sealed {
    pub trait Sealed {}
}

/// The mode of a [`StreamSystem`]: [`Batch`] or [`Live`]. Sealed: the modes
/// are the crate's own.
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

/// The live mode of a [`StreamSystem`]: data arrives in time order, each
/// stream has a frontier before which everything is final, and the processors
/// run incrementally over what became final.
///
/// The mode holds the frontier of every stream and the running state of every
/// processor; make a system in it with [`StreamSystem::live`].
///
/// [`StreamSystem`]: super::StreamSystem
/// [`StreamSystem::live`]: super::StreamSystem::live
#[derive(Debug, Clone)]
pub struct Live {
    /// Where the system started: the frontier of a stream added later.
    pub(super) from: Timestamp,
    /// The frontier of every stream.
    pub(super) frontiers: BTreeMap<Key, Timestamp>,
    /// The running state of each processor, in the order of the pipeline.
    pub(super) nodes: Vec<LiveNode>,
}

/// The running state of one processor.
#[derive(Debug, Clone)]
pub(super) struct LiveNode {
    /// What the recipe compiled into.
    pub(super) recipe: OnlineRecipe,
    /// How far each member it reads has been given to it.
    pub(super) read: BTreeMap<Key, Timestamp>,
}

/// Two live modes are equal when they have the same frontiers: the running
/// state of the processors follows from the data and the frontiers.
impl PartialEq for Live {
    fn eq(&self, other: &Self) -> bool {
        self.from == other.from && self.frontiers == other.frontiers
    }
}

impl sealed::Sealed for Live {}
impl Mode for Live {}
