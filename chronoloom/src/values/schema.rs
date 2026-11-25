//! The shape the values of a member of a system must have.

use core::fmt;
use std::collections::BTreeMap;

use super::value::Value;

static ANY: Schema = Schema::Any;

/// The shape a [`Value`] must have, as data.
///
/// - `Any` accepts every value, `Null`, `Bool`, `Integer`, `Float` and `Text`
///   accept values of that kind, and `Float` also accepts an `Integer`, since a
///   number widens.
/// - `Record(fields)` accepts a record that has every field of `fields`, each
///   of the shape given, **and no other field**: a record rejects unknown
///   fields, so a misspelt name is an error rather than silently ignored. A
///   field whose shape is `Optional` may be left out.
/// - `Optional(shape)` accepts `Null` and whatever `shape` accepts. Only in a
///   record does it also allow the field to be absent; elsewhere it is
///   `shape` or `Null`.
///
/// [`check`] says whether a value has the shape, and when it does not, where:
/// the [`SchemaError`] names the dotted path of the first offending value in
/// field order, and what was expected and found there. A stream system holds a schema
/// for a stream of record values, see [`StreamSystem::set_schema`]. A schema
/// is plain data, so it can be compared, cloned and printed with `Debug`.
///
/// ```
/// use chronoloom::{Schema, Value};
///
/// let message = Schema::record([
///     ("direction", Schema::Text),
///     ("agent", Schema::record([("id", Schema::Integer)])),
///     ("dwell", Schema::optional(Schema::Float)),
/// ]);
///
/// let valid = Value::record([
///     ("direction", Value::from("in")),
///     ("agent", Value::record([("id", Value::from(7))])),
/// ]);
/// assert!(message.check(&valid).is_ok());
///
/// let invalid = Value::record([
///     ("direction", Value::from("in")),
///     ("agent", Value::record([("id", Value::from("seven"))])),
/// ]);
/// let error = message.check(&invalid).unwrap_err();
/// assert_eq!(error.path, "agent.id");
/// assert_eq!((error.expected.as_str(), error.found.as_str()), ("integer", "text"));
/// ```
///
/// [`StreamSystem::set_schema`]: crate::StreamSystem::set_schema
/// [`check`]: Schema::check
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Schema {
    /// Any value.
    Any,
    /// `Null`.
    Null,
    /// A boolean.
    Bool,
    /// An integer.
    Integer,
    /// A float, or an integer.
    Float,
    /// Text.
    Text,
    /// A record with exactly these fields, those that are not `Optional`
    /// required.
    Record(BTreeMap<String, Schema>),
    /// `Null`, or a value of the shape; in a record, a field that may be
    /// absent.
    Optional(Box<Schema>),
}

/// Where a value does not have the shape of a [`Schema`], and how.
///
/// `path` is dotted from the root and empty for the root itself; `expected`
/// describes the shape (`integer`, `optional text`, `record`, or `no such field`
/// for a field the schema does not list), and `found` the value's kind
/// (`text`), or `missing` for a required field that is absent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SchemaError {
    /// The dotted path from the root to the offending value.
    pub path: String,
    /// What the schema asked for there.
    pub expected: String,
    /// What the value was.
    pub found: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = if self.path.is_empty() {
            "the value"
        } else {
            &self.path
        };
        write!(
            f,
            "at `{place}`: expected {}, found {}",
            self.expected, self.found
        )
    }
}

impl std::error::Error for SchemaError {}

impl Schema {
    /// A record schema of the given fields.
    #[must_use]
    pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::Record(
            fields
                .into_iter()
                .map(|(name, schema)| (name.into(), schema))
                .collect(),
        )
    }

    /// `schema`, or `Null`, or (as a field of a record) absent.
    #[must_use]
    pub fn optional(schema: Self) -> Self {
        Self::Optional(Box::new(schema))
    }

    /// Whether `value` has the shape.
    ///
    /// # Errors
    ///
    /// The first offence, in field order, as a [`SchemaError`].
    pub fn check(&self, value: &Value) -> Result<(), SchemaError> {
        self.check_at(value, "")
    }

    /// The shape of the field at the dotted `path`, looking through `Optional`
    /// wrappers; `Any` stands for whatever lies beneath an `Any`. `None` if a
    /// record on the way lacks the field, or the path goes into a shape that is
    /// not a record.
    pub(crate) fn at(&self, path: &str) -> Option<&Self> {
        if path.is_empty() {
            return Some(self);
        }
        path.split('.').try_fold(self, |shape, name| {
            let mut shape = shape;
            while let Self::Optional(inner) = shape {
                shape = inner;
            }
            match shape {
                Self::Record(fields) => fields.get(name),
                Self::Any => Some(&ANY),
                _ => None,
            }
        })
    }

    /// The shape written for a message.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Any => "any value".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Integer => "integer".to_owned(),
            Self::Float => "float".to_owned(),
            Self::Text => "text".to_owned(),
            Self::Record(_) => "record".to_owned(),
            Self::Optional(inner) => format!("optional {}", inner.describe()),
        }
    }

    /// Whether the kind of `value` is one the shape accepts, looking no deeper
    /// than the value itself.
    fn accepts_kind(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Any, _)
            | (Self::Null, Value::Null)
            | (Self::Bool, Value::Bool(_))
            | (Self::Integer, Value::Integer(_))
            | (Self::Float, Value::Float(_) | Value::Integer(_))
            | (Self::Text, Value::Text(_))
            | (Self::Record(_), Value::Record(_)) => true,
            (Self::Optional(inner), value) => {
                matches!(value, Value::Null) || inner.accepts_kind(value)
            }
            _ => false,
        }
    }

    fn check_at(&self, value: &Value, path: &str) -> Result<(), SchemaError> {
        if !self.accepts_kind(value) {
            return Err(SchemaError {
                path: path.to_owned(),
                expected: self.describe(),
                found: value.kind_name().to_owned(),
            });
        }

        match (self, value) {
            (Self::Record(shapes), Value::Record(fields)) => check_record(shapes, fields, path),
            (Self::Optional(inner), value) if !matches!(value, Value::Null) => {
                inner.check_at(value, path)
            }
            _ => Ok(()),
        }
    }
}

/// The path of the field `name` below `path`.
fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

fn check_record(
    shapes: &BTreeMap<String, Schema>,
    fields: &BTreeMap<String, Value>,
    path: &str,
) -> Result<(), SchemaError> {
    for (name, shape) in shapes {
        match fields.get(name) {
            Some(value) => shape.check_at(value, &join(path, name))?,
            None if matches!(shape, Schema::Optional(_)) => {}
            None => {
                return Err(SchemaError {
                    path: join(path, name),
                    expected: shape.describe(),
                    found: "missing".to_owned(),
                })
            }
        }
    }

    match fields.iter().find(|(name, _)| !shapes.contains_key(*name)) {
        Some((name, value)) => Err(SchemaError {
            path: join(path, name),
            expected: "no such field".to_owned(),
            found: value.kind_name().to_owned(),
        }),
        None => Ok(()),
    }
}
