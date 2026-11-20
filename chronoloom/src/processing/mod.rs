//! Formulas and processors: states described as data, and the sets of them
//! that turn the members of a system into new ones.
//!
//! A [`Formula`] says what a state is: active between one kind of event and
//! the next, while a value stays within bounds, or wherever other states are.
//! It is a tree whose leaves read a [`SequenceSystem`] through [`Pattern`]s
//! (a key and a [`Condition`] on the points' values) and whose inner nodes are
//! the set algebra of [`TimeIntervalSequence`]. [`Events`] are the formulas
//! whose result is points, such as the [`Edge`]s of a state. A
//! [`Processor`](crate::processor::Processor) is one recipe built on one, and a
//! [`LiveProcessor`] runs a list of them online.
//!
//! [`SequenceSystem`]: crate::sequence_system::SequenceSystem
//! [`TimeIntervalSequence`]: crate::sequences::TimeIntervalSequence

mod condition;
mod edge;
mod emission;
mod errors;
mod evaluate;
mod events;
mod formula;
mod group;
mod input;
mod kind;
mod live;
mod processor;
#[cfg(feature = "serde")]
mod serialisation;
mod validate;

pub use condition::{Condition, Pattern};
pub use edge::Edge;
pub use emission::{Emission, Emitted};
pub use errors::ProcessError;
pub use events::Events;
pub use formula::{Formula, OpenEnd};
#[cfg(feature = "serde")]
pub(crate) use group::render as entity_name;
pub use group::Group;
pub use input::Input;
pub use kind::Kind;
pub use live::LiveProcessor;
#[cfg(feature = "serde")]
pub(crate) use live::{Checkpoint, Untyped};
pub use processor::Output;
pub(crate) use processor::ProcessorSet;
