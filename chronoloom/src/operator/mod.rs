//! Operators: the crate-private form a formula compiles into.
//!
//! A [`TimelinePlan`] or [`PointsPlan`] is a tree of nodes, one per node of the formula or events, each
//! owning the algorithm that evaluates it over a whole system. The entity
//! recipes compile to plans of their own.
//!
//! Each node also has an online form, an [`OnlineNode`] that steps over the
//! chunks of its inputs as they become final, and whose chunks joined end to
//! end equal the batch result. Live mode will run them; until it does, only
//! the tests do, so the items that nothing else reaches allow dead code
//! outside tests.

mod and;
mod between;
mod changes;
mod chunk;
mod combination;
mod edges;
mod entities;
mod matching;
mod not;
mod occurs;
mod online;
mod or;
mod plan;
mod project;
mod reading;
mod shift;
mod state;
#[cfg(test)]
mod tests;
mod r#while;

pub(crate) use entities::{any_of, count_of};
pub(crate) use plan::{compile_recipe, PointsPlan, TimelinePlan};
