//! The data form of the type a projection reads a field into.

use core::fmt;

/// The type a field of a record is projected into, as data: the kind of
/// sequence an [`Events::Project`] produces.
///
/// A `Kind` stands for a Rust type of the points of the result: `Bool` for
/// `bool`, `Integer` for `i64`, `Float` for `f64` (an integer field widens),
/// `Text` for `String`, and `Value` for a [`Value`], which keeps a sub-record or
/// any field whole. It is data so that a projection can be compared, printed and
/// stored without naming a type parameter.
///
/// [`Events::Project`]: super::Events::Project
/// [`Value`]: crate::Value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Kind {
    /// `bool` points.
    Bool,
    /// `i64` points.
    Integer,
    /// `f64` points.
    Float,
    /// `String` points.
    Text,
    /// `Value` points.
    Value,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bool => "bool",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Text => "text",
            Self::Value => "value",
        })
    }
}
