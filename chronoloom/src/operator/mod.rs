//! Operators: the crate-private form a formula compiles into.
//!
//! A [`TimelinePlan`] or [`PointsPlan`] is a tree of nodes, one per node of the formula or events, each
//! owning the algorithm that evaluates it over a whole system. The entity
//! recipes still call the evaluation of the processing module.

mod and;
mod between;
mod changes;
mod edges;
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

pub(crate) use plan::{compile_recipe, PointsPlan, TimelinePlan};
pub(crate) use reading::records;
