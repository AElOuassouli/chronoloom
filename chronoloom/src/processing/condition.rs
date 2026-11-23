//! What a single point must satisfy, and the pattern that says which points.

use core::any::{self, Any};
use core::fmt;

use crate::primitives::Timestamp;
use crate::sequence_system::{AnyPointSequence, Key, PointValue};
use crate::sequences::TimePointSequence;
use crate::values::Value;

/// What a point's value must satisfy, as data over the value type `T`.
///
/// A condition is a tree: the leaves compare the value with constants, and
/// `And`, `Or` and `Not` combine conditions. It is plain data, so it can be
/// compared, cloned, printed and, later, stored, and [`matches`] is the one
/// place that gives it meaning.
///
/// `Within { low, high }` is half-open, `low <= value < high`, like a span.
/// The ordering conditions (`Less`, `LessOrEqual`, `Greater`, `GreaterOrEqual`
/// and `Within`) need `T: PartialOrd`, and a value that does not compare with
/// the constant, such as a NaN, matches none of them. `Equals`, `NotEquals`
/// and `In` use `==` and `!=`, so for a NaN `Equals` and `In` do not match and
/// `NotEquals` does. An empty `And` matches every value and an empty `Or`
/// matches none.
///
/// [`Display`] writes the condition in the notation the documentation uses,
/// about a subject called `value`: `value > 5`, `5 <= value < 10`,
/// `value = "start"`. A [`Pattern`] writes the same about the key it reads.
///
/// ```
/// use chronoloom::Condition;
///
/// let moderate = Condition::And(vec![Condition::GreaterOrEqual(5), Condition::Less(10)]);
/// assert!(moderate.matches(&5) && !moderate.matches(&10));
/// assert_eq!(moderate.to_string(), "value >= 5 ∧ value < 10");
///
/// let range = Condition::Within { low: 5, high: 10 };
/// assert!(range.matches(&9) && !range.matches(&10));
/// assert_eq!(range.to_string(), "5 <= value < 10");
/// ```
///
/// [`Display`]: core::fmt::Display
/// [`matches`]: Condition::matches
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Condition<T> {
    /// Every value.
    Any,
    /// A value equal to the constant.
    Equals(T),
    /// A value not equal to the constant.
    NotEquals(T),
    /// A value below the constant.
    Less(T),
    /// A value at or below the constant.
    LessOrEqual(T),
    /// A value above the constant.
    Greater(T),
    /// A value at or above the constant.
    GreaterOrEqual(T),
    /// A value from `low` up to, and excluding, `high`.
    Within {
        /// The lowest value that matches.
        low: T,
        /// The first value above the range, which does not match.
        high: T,
    },
    /// A value equal to one of the constants.
    In(Vec<T>),
    /// A value that satisfies every condition.
    And(Vec<Condition<T>>),
    /// A value that satisfies at least one condition.
    Or(Vec<Condition<T>>),
    /// A value that does not satisfy the condition.
    Not(Box<Condition<T>>),
}

impl<T: PartialOrd> Condition<T> {
    /// Whether `value` satisfies the condition.
    #[must_use]
    pub fn matches(&self, value: &T) -> bool {
        match self {
            Self::Any => true,
            Self::Equals(constant) => value == constant,
            Self::NotEquals(constant) => value != constant,
            Self::Less(constant) => value < constant,
            Self::LessOrEqual(constant) => value <= constant,
            Self::Greater(constant) => value > constant,
            Self::GreaterOrEqual(constant) => value >= constant,
            Self::Within { low, high } => value >= low && value < high,
            Self::In(constants) => constants.contains(value),
            Self::And(conditions) => conditions.iter().all(|condition| condition.matches(value)),
            Self::Or(conditions) => conditions.iter().any(|condition| condition.matches(value)),
            Self::Not(condition) => !condition.matches(value),
        }
    }
}

impl<T: fmt::Debug> Condition<T> {
    /// Write the condition about `subject`.
    fn write(&self, subject: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any {subject}"),
            Self::Equals(constant) => write!(f, "{subject} = {constant:?}"),
            Self::NotEquals(constant) => write!(f, "{subject} != {constant:?}"),
            Self::Less(constant) => write!(f, "{subject} < {constant:?}"),
            Self::LessOrEqual(constant) => write!(f, "{subject} <= {constant:?}"),
            Self::Greater(constant) => write!(f, "{subject} > {constant:?}"),
            Self::GreaterOrEqual(constant) => write!(f, "{subject} >= {constant:?}"),
            Self::Within { low, high } => write!(f, "{low:?} <= {subject} < {high:?}"),
            Self::In(constants) => {
                write!(f, "{subject} in {{")?;
                for (index, constant) in constants.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{constant:?}")?;
                }
                f.write_str("}")
            }
            Self::And(conditions) => Self::write_all(conditions, "∧", "⊤", subject, f),
            Self::Or(conditions) => Self::write_all(conditions, "∨", "⊥", subject, f),
            Self::Not(condition) => {
                f.write_str("¬(")?;
                condition.write(subject, f)?;
                f.write_str(")")
            }
        }
    }

    /// Write `conditions` joined by `symbol`, grouping the nested ones.
    fn write_all(
        conditions: &[Self],
        symbol: &str,
        empty: &str,
        subject: &str,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        if conditions.is_empty() {
            return f.write_str(empty);
        }

        for (index, condition) in conditions.iter().enumerate() {
            if index > 0 {
                write!(f, " {symbol} ")?;
            }
            if matches!(condition, Self::And(_) | Self::Or(_)) {
                f.write_str("(")?;
                condition.write(subject, f)?;
                f.write_str(")")?;
            } else {
                condition.write(subject, f)?;
            }
        }

        Ok(())
    }
}

impl<T: fmt::Debug> fmt::Display for Condition<T> {
    /// The condition about a subject called `value`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write("value", f)
    }
}

/// A condition with its value type erased, so patterns over different value
/// types can sit in one formula. Private: [`Pattern`] is the public face.
pub(crate) trait ErasedCondition: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn ErasedCondition>;
    /// Equal when `other` is over the same value type and an equal condition.
    fn equals(&self, other: &dyn ErasedCondition) -> bool;
    fn write(&self, subject: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn value_type_name(&self) -> &'static str;
    /// The timestamp of each point of `points` in order, with whether its value
    /// matches; `None` when the points carry another value type.
    fn marks(&self, points: &AnyPointSequence) -> Option<Vec<(Timestamp, bool)>>;
    /// The matching points of `points`, values kept; `None` when the points
    /// carry another value type.
    fn matching(&self, points: &AnyPointSequence) -> Option<AnyPointSequence>;
    /// A point sequence with no events, of the value type the condition is
    /// over.
    fn empty_points(&self, label: &str) -> AnyPointSequence;
}

impl<T: PointValue + PartialOrd> ErasedCondition for Condition<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedCondition> {
        Box::new(self.clone())
    }

    fn equals(&self, other: &dyn ErasedCondition) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }

    fn write(&self, subject: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Self::write(self, subject, f)
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<T>()
    }

    fn marks(&self, points: &AnyPointSequence) -> Option<Vec<(Timestamp, bool)>> {
        marks_with(points, |value| self.matches(value))
    }

    fn empty_points(&self, label: &str) -> AnyPointSequence {
        TimePointSequence::<T>::new(label).into()
    }

    fn matching(&self, points: &AnyPointSequence) -> Option<AnyPointSequence> {
        matching_with(points, |value| self.matches(value))
    }
}

/// The timestamp of each point of `points`, with whether `accepts` holds for
/// its value; `None` when the points carry another value type than `T`.
fn marks_with<T: PointValue>(
    points: &AnyPointSequence,
    accepts: impl Fn(&T) -> bool,
) -> Option<Vec<(Timestamp, bool)>> {
    let typed = points.downcast_ref::<T>()?;

    Some(
        typed
            .iter()
            .map(|event| (event.timestamp(), accepts(event.value())))
            .collect(),
    )
}

/// The points of `points` whose value `accepts`, values and extent kept;
/// `None` when the points carry another value type than `T`.
fn matching_with<T: PointValue>(
    points: &AnyPointSequence,
    accepts: impl Fn(&T) -> bool,
) -> Option<AnyPointSequence> {
    let typed = points.downcast_ref::<T>()?;
    let events = typed
        .iter()
        .filter(|event| accepts(event.value()))
        .cloned()
        .collect();

    Some(
        TimePointSequence::from_parts(typed.attribute().clone(), events, typed.observed().clone())
            .into(),
    )
}

/// A condition on the field of a record value at a dotted path: the erased
/// condition behind [`Pattern::field`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldCondition {
    pub(crate) path: String,
    pub(crate) condition: Condition<Value>,
}

impl FieldCondition {
    /// Whether the field of `value` at the path exists and satisfies the
    /// condition; a missing field never does.
    fn accepts(&self, value: &Value) -> bool {
        value
            .get(&self.path)
            .is_some_and(|field| self.condition.matches(field))
    }
}

impl ErasedCondition for FieldCondition {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedCondition> {
        Box::new(self.clone())
    }

    fn equals(&self, other: &dyn ErasedCondition) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }

    fn write(&self, subject: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            self.condition.write(subject, f)
        } else {
            self.condition.write(&format!("{subject}.{}", self.path), f)
        }
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<Value>()
    }

    fn marks(&self, points: &AnyPointSequence) -> Option<Vec<(Timestamp, bool)>> {
        marks_with(points, |value| self.accepts(value))
    }

    fn matching(&self, points: &AnyPointSequence) -> Option<AnyPointSequence> {
        matching_with(points, |value| self.accepts(value))
    }

    fn empty_points(&self, label: &str) -> AnyPointSequence {
        TimePointSequence::<Value>::new(label).into()
    }
}

/// Which points a formula reads: those of the member under `key` whose value
/// satisfies a [`Condition`].
///
/// The condition is over the value type of the member it reads, and each
/// pattern is typed by its own member: a formula may read `f64` samples
/// through one pattern and `String` events through another. The value type is
/// erased inside the pattern, the way [`AnyPointSequence`] erases a point
/// sequence's, and checked against the member when the formula is evaluated,
/// where a mismatch is [`ProcessError::ValueTypeMismatch`], never a silent
/// "no match". A pattern keeps `Debug`, `Clone` and `PartialEq` (patterns over
/// different value types are never equal), and [`Display`] writes the
/// condition about the key: `motion = "start"`, `5 <= count < 10`.
///
/// ```
/// use chronoloom::{Condition, Pattern};
///
/// let busy = Pattern::new("count", Condition::<i64>::Greater(5));
/// assert_eq!(busy.to_string(), "count > 5");
/// assert_eq!(busy, Pattern::new("count", Condition::Greater(5_i64)));
/// assert_ne!(busy, Pattern::new("count", Condition::Greater(5.0_f64)));
/// ```
///
/// # Field patterns
///
/// [`Pattern::field`] reads a member of record [`Value`]s through one of their
/// fields, at a dotted path as [`Value::get`] reads it, and applies a
/// `Condition<Value>` to that field: `"direction"` is `"in"`, `"count"` is
/// above five. It works wherever a pattern does (`Between`, `While`, `Occurs`,
/// `hold`, `Events::Matching`), in batch and online, and is written
/// `motion.direction = "in"`: the key, a dot and the path. Two field patterns
/// are equal when they read the same key and path with an equal condition. A
/// condition over two fields is two field patterns combined as formulas, an
/// `And` of two `While`s.
///
/// - A point whose record has no field at the path does not match, whatever
///   the condition (`Any` and `NotEquals` included). That is not an error. For
///   `While` and `hold` such a point is a sample that does not satisfy the
///   condition, so it ends a span, where `Events::Project` skips it.
/// - A field that is `Null` is a value like any other: the condition is
///   applied to it, so `Equals(Value::Null)` and `Any` match it, and the
///   orderings do not, as `Null` is unordered with everything else. This is
///   what a plain pattern reads from a member projected with `Kind::Value`,
///   which keeps nulls; a projection into any other kind skips them.
/// - Numbers compare as numbers across `Integer` and `Float`, so
///   `Greater(Value::from(5))` matches `5.5`.
/// - Only a member of `Value` points can be read: any other member is
///   [`ProcessError::ValueTypeMismatch`], in batch and when a live processor
///   is built.
///
/// `Between` and `While` over a field equal the same formulas over the field
/// projected with `Kind::Value` and read with a plain pattern, provided every
/// event has the field. They differ only for an event without it: the
/// projection drops that event, so its absence changes nothing, while the
/// field pattern reads it as a sample that does not match, which ends a
/// `While` span and counts as no `start` or `end` of a `Between`.
///
/// ```
/// use chronoloom::stream_system::StreamSystem;
/// use chronoloom::{
///     Condition, Formula, Pattern, Processor, TimePointEvent,
///     TimePointSequence, Value,
/// };
///
/// let reading = |count: i64| Value::record([("count", Value::from(count))]);
/// let mut system = StreamSystem::new();
/// system.insert_stream("motion", TimePointSequence::from_events("motion", vec![
///     TimePointEvent::new(10, reading(7)),
///     TimePointEvent::new(20, Value::record([("zone", Value::from("a"))])),
///     TimePointEvent::new(30, reading(2)),
/// ]))?;
///
/// let busy = Pattern::field("motion", "count", Condition::Greater(Value::from(5)));
/// assert_eq!(busy.to_string(), "motion.count > 5");
///
/// // The reading without a count does not match, so it ends the span.
/// let busy = Processor::new("busy", Formula::while_matching(busy)).evaluate(&system)?;
/// let busy = busy.as_time_interval().unwrap();
/// let spans: Vec<(i64, i64)> = busy
///     .iter()
///     .map(|span| (span.start(), span.end()))
///     .collect();
/// assert_eq!(spans, [(10, 20)]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// [`Display`]: core::fmt::Display
/// [`ProcessError::ValueTypeMismatch`]: super::ProcessError::ValueTypeMismatch
/// [`Value`]: crate::Value
/// [`Value::get`]: crate::Value::get
pub struct Pattern {
    pub(crate) key: Key,
    pub(crate) condition: Box<dyn ErasedCondition>,
}

impl Pattern {
    /// The points of the member under `key` whose value satisfies `condition`.
    #[must_use]
    pub fn new<T: PointValue + PartialOrd>(key: impl Into<Key>, condition: Condition<T>) -> Self {
        Self {
            key: key.into(),
            condition: Box::new(condition),
        }
    }

    /// The points of the member under `key`, a member of record [`Value`]s,
    /// whose field at the dotted `path` exists and satisfies `condition`.
    ///
    #[must_use]
    pub fn field(
        key: impl Into<Key>,
        path: impl Into<String>,
        condition: Condition<Value>,
    ) -> Self {
        Self {
            key: key.into(),
            condition: Box::new(FieldCondition {
                path: path.into(),
                condition,
            }),
        }
    }

    /// The key of the member the pattern reads.
    #[must_use]
    pub const fn key(&self) -> &Key {
        &self.key
    }

    /// The name of the value type the condition is over, as
    /// [`core::any::type_name`] spells it; meant for messages.
    #[must_use]
    pub fn value_type_name(&self) -> &'static str {
        self.condition.value_type_name()
    }
}

impl Clone for Pattern {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            condition: self.condition.clone_box(),
        }
    }
}

impl fmt::Debug for Pattern {
    /// The typed condition's own `Debug`, as though it had never been erased.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pattern")
            .field("key", &self.key)
            .field("condition", &self.condition)
            .finish()
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.condition.equals(&*other.condition)
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.condition.write(self.key.as_str(), f)
    }
}
