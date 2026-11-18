//! A [`Value`] as plain data: `null`, `true`, `5`, `5.0`, `"text"` and `{..}`.

use core::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::{Error as _, SerializeMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::values::Value;

impl Serialize for Value {
    /// # Errors
    ///
    /// Refuses a float that is not finite, which JSON cannot hold.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(flag) => serializer.serialize_bool(*flag),
            Self::Integer(number) => serializer.serialize_i64(*number),
            Self::Float(number) if number.is_finite() => serializer.serialize_f64(*number),
            Self::Float(number) => Err(S::Error::custom(format_args!(
                "a float that is not finite ({number}) has no form in the format"
            ))),
            Self::Text(text) => serializer.serialize_str(text),
            Self::Record(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (name, field) in fields {
                    map.serialize_entry(name, field)?;
                }
                map.end()
            }
        }
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("null, a bool, a number, a string or a record")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, flag: bool) -> Result<Value, E> {
        Ok(Value::Bool(flag))
    }

    fn visit_i64<E: de::Error>(self, number: i64) -> Result<Value, E> {
        Ok(Value::Integer(number))
    }

    fn visit_u64<E: de::Error>(self, number: u64) -> Result<Value, E> {
        i64::try_from(number)
            .map(Value::Integer)
            .map_err(|_| E::custom(format_args!("the integer {number} does not fit in an i64")))
    }

    fn visit_f64<E: de::Error>(self, number: f64) -> Result<Value, E> {
        Ok(Value::Float(number))
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Value, E> {
        Ok(Value::Text(text.to_owned()))
    }

    fn visit_string<E: de::Error>(self, text: String) -> Result<Value, E> {
        Ok(Value::Text(text))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut fields = std::collections::BTreeMap::new();
        while let Some((name, field)) = map.next_entry::<String, Value>()? {
            if fields.contains_key(&name) {
                return Err(de::Error::custom(format_args!(
                    "the field `{name}` appears twice in a record"
                )));
            }
            fields.insert(name, field);
        }

        Ok(Value::Record(fields))
    }
}

impl<'de> Deserialize<'de> for Value {
    /// An integer in the document is an [`Integer`](Value::Integer) and a
    /// number with a fraction or an exponent is a [`Float`](Value::Float).
    ///
    /// # Errors
    ///
    /// Refuses a list, and an integer beyond `i64`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}
