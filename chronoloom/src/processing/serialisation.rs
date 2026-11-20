//! A [`Pattern`] as data: its key, the type of the values it reads, and its
//! condition. The types are the closed set of the
//! [`serialisation`](crate::serialisation) module.

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};

use crate::sequence_system::Key;
use crate::values::Value;

use super::condition::{Condition, FieldCondition, Pattern};
use super::edge::Edge;

/// The condition of a pattern, tagged with the type of the values it reads.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename = "Condition")]
enum ConditionRef<'a> {
    F64 { condition: &'a Condition<f64> },
    I64 { condition: &'a Condition<i64> },
    Bool { condition: &'a Condition<bool> },
    Text { condition: &'a Condition<String> },
    Value { condition: &'a Condition<Value> },
    Edge { condition: &'a Condition<Edge> },
}

#[derive(Serialize)]
#[serde(rename = "Pattern")]
struct PatternRef<'a> {
    key: &'a Key,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    #[serde(flatten)]
    condition: ConditionRef<'a>,
}

impl Serialize for Pattern {
    /// # Errors
    ///
    /// Refuses a pattern on a value type outside the closed set.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let any = self.condition.as_any();
        let (path, condition) = if let Some(field) = any.downcast_ref::<FieldCondition>() {
            (
                Some(field.path.as_str()),
                Some(ConditionRef::Value {
                    condition: &field.condition,
                }),
            )
        } else {
            (
                None,
                any.downcast_ref::<Condition<f64>>()
                    .map(|condition| ConditionRef::F64 { condition })
                    .or_else(|| {
                        any.downcast_ref()
                            .map(|condition| ConditionRef::I64 { condition })
                    })
                    .or_else(|| {
                        any.downcast_ref()
                            .map(|condition| ConditionRef::Bool { condition })
                    })
                    .or_else(|| {
                        any.downcast_ref()
                            .map(|condition| ConditionRef::Text { condition })
                    })
                    .or_else(|| {
                        any.downcast_ref()
                            .map(|condition| ConditionRef::Value { condition })
                    })
                    .or_else(|| {
                        any.downcast_ref()
                            .map(|condition| ConditionRef::Edge { condition })
                    }),
            )
        };
        let condition = condition.ok_or_else(|| {
            ser::Error::custom(format_args!(
                "the pattern on `{}` reads `{}` values, which the format does not support: it \
                 supports f64, i64, bool, String, Value and Edge",
                self.key,
                self.condition.value_type_name()
            ))
        })?;

        PatternRef {
            key: &self.key,
            path,
            condition,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConditionBody {
    F64 {
        condition: Condition<f64>,
    },
    I64 {
        condition: Condition<i64>,
    },
    Bool {
        condition: Condition<bool>,
    },
    Text {
        condition: Condition<String>,
    },
    Value {
        condition: Condition<Value>,
    },
    Edge {
        condition: Condition<Edge>,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Deserialize)]
struct PatternBody {
    key: Key,
    #[serde(default)]
    path: Option<String>,
    #[serde(flatten)]
    condition: ConditionBody,
}

impl<'de> Deserialize<'de> for Pattern {
    /// # Errors
    ///
    /// Refuses a value type outside the closed set, and a `path` on a pattern
    /// that does not read `Value`s.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let PatternBody {
            key,
            path,
            condition,
        } = PatternBody::deserialize(deserializer)?;

        match (condition, path) {
            (ConditionBody::Value { condition }, Some(path)) => {
                Ok(Self::field(key, path, condition))
            }
            (_, Some(_)) => Err(de::Error::custom(format_args!(
                "the pattern on `{key}` has a path, which only a pattern on `value` types can have"
            ))),
            (ConditionBody::F64 { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::I64 { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::Bool { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::Text { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::Value { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::Edge { condition }, None) => Ok(Self::new(key, condition)),
            (ConditionBody::Unsupported, _) => Err(de::Error::custom(format_args!(
                "the pattern on `{key}` reads an unsupported value type: the format supports \
                 f64, i64, bool, text, value and edge"
            ))),
        }
    }
}
