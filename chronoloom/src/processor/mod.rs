//! A processor: one recipe, the inputs it reads, a formula over them, and the
//! one output it produces.
//!
//! A [`Processor`] is used by a [`StreamSystem`], which keeps its output up to
//! date, and is a value of its own besides.
//!
//! [`StreamSystem`]: crate::stream_system::StreamSystem

mod definition;

pub use definition::Processor;
