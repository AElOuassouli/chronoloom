//! Operators: the crate-private form a formula compiles into.
//!
//! A [`TimelinePlan`] or [`PointsPlan`] is a tree of nodes, one per node of the formula or events, each
//! owning the algorithm that evaluates it over a whole system. The entity
//! recipes compile to plans of their own, whose online form keeps one state of
//! the formula per entity.
//!
//! Each node also has an online form, an [`OnlineNode`] that steps over the
//! chunks of its inputs as they become final, and whose chunks joined end to
//! end equal the batch result. Live mode runs them.

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
mod online_entities;
mod online_points;
mod online_recipe;
mod or;
mod plan;
mod project;
mod reading;
mod shift;
mod state;
#[cfg(test)]
mod tests;
mod r#while;

pub use chunk::Chunk;
pub(crate) use chunk::Inputs;
pub(crate) use entities::{any_of, count_of};
pub(crate) use online_recipe::OnlineRecipe;
pub(crate) use plan::{compile_recipe, TimelinePlan};
