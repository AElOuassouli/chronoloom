//! Formulas and processors: states described as data, and the sets of them
//! that turn the members of a system into new ones.
//!
//! A [`Formula`] says what a state is: active between one kind of event and
//! the next, while a value stays within bounds, or wherever other states are.
//! It is a tree whose leaves read a [`SequenceSystem`] through [`Pattern`]s
//! (a key and a [`Condition`] on the points' values) and whose inner nodes are
//! the set algebra of [`TimeIntervalSequence`]. [`Events`] are the formulas
//! whose result is points, such as the [`Edge`]s of a state. A [`Processor`]
//! is a set of them, each filed under the key of the member it produces.
//!
//! [`SequenceSystem`]: crate::sequence_system::SequenceSystem
//! [`TimeIntervalSequence`]: crate::sequences::TimeIntervalSequence

mod condition;
mod edge;
mod errors;
mod evaluate;
mod events;
mod formula;
mod processor;

pub use condition::{Condition, Pattern};
pub use edge::Edge;
pub use errors::ProcessError;
pub use events::Events;
pub use formula::{Formula, OpenEnd};
pub use processor::{Output, Processor};
