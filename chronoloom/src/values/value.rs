//! The dynamic value a point of a record sequence carries.

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeMap;

/// A dynamic value: null, a boolean, an integer, a float, text, or a record of
/// named fields, each a value in turn.
///
/// `TimePointSequence<Value>` is the sequence of structured payloads, such as a
/// sensor message with a direction, an agent and a dwell time, beside the
/// sequences of one fixed value type (`f64`, `String`, ...), which keep their own
/// dense layout. A `Value` is a [`PointValue`](crate::PointValue) like any other
/// type that is `Clone`, `Debug` and `PartialEq`, so it is recorded into a
/// system and read by processors as is; [`Events::Project`] reads a field of
/// each record out into a sequence of its own, and a [`Schema`] says what shape
/// the records of a member must have.
///
/// ```
/// use chronoloom::Value;
///
/// let message = Value::record([
///     ("direction", Value::from("in")),
///     ("agent", Value::record([("id", Value::from(7)), ("speed", Value::from(1.5))])),
/// ]);
///
/// assert_eq!(message.get("agent.id"), Some(&Value::Integer(7)));
/// assert_eq!(message.get("agent.id").and_then(Value::as_float), Some(7.0));
/// assert_eq!(message.get("agent.name"), None);
/// assert_eq!(
///     message.to_string(),
///     r#"{"agent": {"id": 7, "speed": 1.5}, "direction": "in"}"#,
/// );
/// ```
///
/// # Paths
///
/// [`get`] reaches a field through a dotted path: `"agent.id"` is the field
/// `id` of the record in the field `agent`. The empty path is the value itself.
/// A segment is a field name taken literally, so a name that contains a dot
/// cannot be reached by a path, an empty segment names the field `""`, and a
/// path through a value that is not a record finds nothing.
///
/// # Ordering and equality
///
/// Values of one kind compare naturally: `false < true`, integers and floats by
/// number, text by its bytes, and `null` equal to `null`. An integer and a float
/// compare as numbers, exactly, with no rounding of large integers, and are
/// equal when they denote the same number: `Value::from(5) == Value::from(5.0)`.
/// Equality follows ordering, so a `NaN` is equal to nothing, itself included,
/// which is why `Value` is `PartialEq` and not `Eq`. Records are compared for
/// equality, field by field, and are otherwise unordered; and any other pair of
/// kinds is unordered, so a comparison between them is neither less, equal nor
/// greater. Hence a [`Condition<Value>`] with an ordering such as
/// `Greater(Value::from(5))` matches the numbers above five, and simply does not
/// match text or records, without error.
///
/// # Display
///
/// [`Display`](fmt::Display) writes the value as JSON would, with the fields of
/// a record in key order and a space after each colon and comma, so that it
/// reads well in a formula label: `null`, `true`, `7`, `1.5`, `"in"`,
/// `{"id": 7}`. A float always shows a point or an exponent (`5.0`), so it
/// reads apart from an integer, and a `NaN` or an infinity is written as
/// `NaN`, `inf` or `-inf`, which JSON cannot express. Text is escaped as JSON
/// escapes it.
///
/// [`Debug`](fmt::Debug) writes the same text, so that a condition or a pattern
/// over values, which a formula label writes with `Debug`, reads as
/// `value > 5` and `r = {"n": 1}` rather than `value > Integer(5)`.
///
/// [`Condition<Value>`]: crate::Condition
/// [`Events::Project`]: crate::Events::Project
/// [`Schema`]: crate::Schema
/// [`get`]: Value::get
#[derive(Clone)]
pub enum Value {
    /// No value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Integer(i64),
    /// A floating point number.
    Float(f64),
    /// Text.
    Text(String),
    /// Named fields, in key order.
    Record(BTreeMap<String, Value>),
}

impl Value {
    /// A record of the given fields; a name given twice keeps its last value.
    #[must_use]
    pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::Record(
            fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )
    }

    /// The value at `path`, as the type documentation describes paths, or
    /// `None` if there is no such field.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&Self> {
        if path.is_empty() {
            return Some(self);
        }

        path.split('.').try_fold(self, |value, name| match value {
            Self::Record(fields) => fields.get(name),
            _ => None,
        })
    }

    /// The name of the value's kind: `null`, `bool`, `integer`, `float`, `text`
    /// or `record`; meant for messages.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::Text(_) => "text",
            Self::Record(_) => "record",
        }
    }

    /// The boolean, if the value is one.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The integer, if the value is one.
    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// The number as a float, if the value is a float or an integer; a large
    /// integer is rounded to the nearest float.
    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }

    /// The text, if the value is text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    /// The fields, if the value is a record.
    #[must_use]
    pub const fn as_record(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Record(fields) => Some(fields),
            _ => None,
        }
    }
}

/// How an integer compares with a float, exactly; `None` for a `NaN`.
fn compare_integer_with_float(integer: i64, float: f64) -> Option<Ordering> {
    // 2^63, the first float above every integer.
    const BOUND: f64 = 9_223_372_036_854_775_808.0;

    if float.is_nan() {
        None
    } else if float >= BOUND {
        Some(Ordering::Less)
    } else if float < -BOUND {
        Some(Ordering::Greater)
    } else {
        // In range, so the whole part is an exact integer.
        let whole = float.trunc();
        let fraction = float - whole;
        Some(
            integer
                .cmp(&(whole as i64))
                .then(0.0_f64.partial_cmp(&fraction).unwrap_or(Ordering::Equal)),
        )
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Record(first), Self::Record(second)) => first == second,
            _ => self.partial_cmp(other) == Some(Ordering::Equal),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Null, Self::Null) => Some(Ordering::Equal),
            (Self::Bool(first), Self::Bool(second)) => first.partial_cmp(second),
            (Self::Integer(first), Self::Integer(second)) => first.partial_cmp(second),
            (Self::Float(first), Self::Float(second)) => first.partial_cmp(second),
            (Self::Integer(integer), Self::Float(float)) => {
                compare_integer_with_float(*integer, *float)
            }
            (Self::Float(float), Self::Integer(integer)) => {
                compare_integer_with_float(*integer, *float).map(Ordering::reverse)
            }
            (Self::Text(first), Self::Text(second)) => first.partial_cmp(second),
            (Self::Record(first), Self::Record(second)) => {
                (first == second).then_some(Ordering::Equal)
            }
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<BTreeMap<String, Self>> for Value {
    fn from(fields: BTreeMap<String, Self>) -> Self {
        Self::Record(fields)
    }
}

/// Write `text` between quotes, escaped as JSON escapes it.
fn write_quoted(text: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("\"")?;
    for character in text.chars() {
        match character {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            control if control < ' ' => write!(f, "\\u{:04x}", u32::from(control))?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Integer(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value:?}"),
            Self::Text(text) => write_quoted(text, f),
            Self::Record(fields) => {
                f.write_str("{")?;
                for (index, (name, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write_quoted(name, f)?;
                    write!(f, ": {value}")?;
                }
                f.write_str("}")
            }
        }
    }
}
