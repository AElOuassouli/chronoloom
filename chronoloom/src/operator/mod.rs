//! Operators: the crate-private form a formula compiles into.
//!
//! A [`TimelinePlan`] or [`PointsPlan`] is a tree of nodes, one per node of the formula or events, each
//! owning the algorithm that evaluates it over a whole system. The entity
//! recipes compile to plans of their own.

mod and;
mod between;
mod changes;
mod edges;
mod entities;
mod matching;
mod not;
mod occurs;
mod or;
mod plan;
mod project;
mod reading;
mod shift;
mod state;
mod r#while;

pub(crate) use entities::{any_of, count_of};
pub(crate) use plan::{compile_recipe, PointsPlan, TimelinePlan};
