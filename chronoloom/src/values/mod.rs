//! Record values: a dynamic [`Value`] beside the generic value type of a
//! sequence, and the [`Schema`] a member of a system may require of its values.

mod schema;
mod value;

pub use schema::{Schema, SchemaError};
pub use value::Value;
