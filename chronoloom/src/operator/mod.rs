//! Operators: the crate-private form a formula compiles into.
//!
//! A [`Plan`] is a tree of nodes, one per node of the formula, each owning the
//! algorithm that evaluates it over a whole system. Nodes not yet moved here
//! call the evaluation of the processing module.

mod and;
mod not;
mod occurs;
mod or;
mod plan;
mod state;

pub(crate) use plan::Plan;
