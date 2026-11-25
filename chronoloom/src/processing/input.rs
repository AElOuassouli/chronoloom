//! What a processor expects of one of its inputs, as data.

use core::any::type_name;
use core::fmt;

use crate::store::{Key, Sequence};
use crate::values::{Schema, Value};

use super::edge::Edge;
use super::errors::ProcessError;
use super::kind::Kind;

/// What a [`Processor`] expects of the member it reads under a key: a
/// timeline, or points of one value type, and for records the shape of their
/// values.
///
/// The value types are those of [`Kind`] (`bool`, `i64`, `f64`, `String` and
/// [`Value`]) and [`Edge`], which is not a kind a projection can produce. An
/// `Input` is declared with [`Processor::with_input`]; the narrative of what a
/// declaration changes is on [`Processor`].
///
/// - `Timeline` is a timeline.
/// - `Points(kind)` is points of the type `kind` stands for, with no
///   requirement on their values.
/// - `Edges` is points of [`Edge`]s.
/// - `Records(schema)` is points of [`Value`]s that all have the shape
///   `schema`, which a processor can then check its field patterns,
///   projections and groups against before it runs.
///
/// [`Processor`]: crate::processor::Processor
/// [`Processor::with_input`]: crate::processor::Processor::with_input
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Input {
    /// A timeline.
    Timeline,
    /// Points of one value type.
    Points(Kind),
    /// Points of [`Edge`]s.
    Edges,
    /// Points of [`Value`]s, all of this shape.
    Records(Schema),
}

impl Input {
    /// The name of the value type of the points, as [`core::any::type_name`]
    /// spells it, or `None` for a timeline.
    pub(crate) fn value_type_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::Timeline => return None,
            Self::Points(Kind::Bool) => type_name::<bool>(),
            Self::Points(Kind::Integer) => type_name::<i64>(),
            Self::Points(Kind::Float) => type_name::<f64>(),
            Self::Points(Kind::Text) => type_name::<String>(),
            Self::Points(Kind::Value) | Self::Records(_) => type_name::<Value>(),
            Self::Edges => type_name::<Edge>(),
        })
    }

    /// The shape of the values, for records.
    pub(crate) const fn schema(&self) -> Option<&Schema> {
        match self {
            Self::Records(schema) => Some(schema),
            _ => None,
        }
    }

    /// Whether `sequence`, the member filed under `key`, is what the input
    /// declares.
    pub(crate) fn check(&self, key: &Key, sequence: &Sequence) -> Result<(), ProcessError> {
        let Some(expected) = self.value_type_name() else {
            return match sequence {
                Sequence::TimeInterval(_) => Ok(()),
                Sequence::TimePoint(_) => Err(ProcessError::NotATimeline { key: key.clone() }),
            };
        };
        let Sequence::TimePoint(points) = sequence else {
            return Err(ProcessError::NotPoints { key: key.clone() });
        };
        if points.value_type_name() != expected {
            return Err(ProcessError::ValueTypeMismatch {
                key: key.clone(),
                expected: points.value_type_name(),
                found: expected,
            });
        }

        if let (Some(schema), Some(records)) = (self.schema(), points.downcast_ref::<Value>()) {
            if let Some(error) = records
                .iter()
                .find_map(|event| schema.check(event.value()).err())
            {
                return Err(ProcessError::SchemaMismatch {
                    key: key.clone(),
                    error,
                });
            }
        }

        Ok(())
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeline => f.write_str("a timeline"),
            Self::Points(kind) => write!(f, "{kind} points"),
            Self::Edges => f.write_str("edge points"),
            Self::Records(_) => f.write_str("record points"),
        }
    }
}
