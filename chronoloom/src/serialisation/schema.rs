//! The JSON Schema of the processor document, behind the `json-schema`
//! feature.
//!
//! Most types derive their schema from the same `serde` attributes that shape
//! their JSON. A [`Value`] and a [`Pattern`] have hand-written `serde` forms, so
//! they have hand-written schemas that describe what is actually written.

use std::borrow::Cow;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};

use crate::processing::{Condition, Edge, Input, Output, Pattern};
use crate::processor::Processor;
use crate::sequence_system::Key;
use crate::values::Value;

impl JsonSchema for Value {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Value")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "null, a boolean, a number, text, or a record of named values",
            "type": ["null", "boolean", "number", "string", "object"],
            "additionalProperties": generator.subschema_for::<Self>()
        })
    }
}

impl JsonSchema for Pattern {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Pattern")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let key = generator.subschema_for::<Key>();
        let typed = |tag: &str, condition: Schema, with_path: bool| {
            let mut variant = json_schema!({
                "properties": {
                    "type": {"const": tag},
                    "condition": condition
                }
            });
            if !with_path {
                variant.insert("not".to_owned(), serde_json::json!({"required": ["path"]}));
            }
            variant
        };
        let variants = [
            typed("f64", generator.subschema_for::<Condition<f64>>(), false),
            typed("i64", generator.subschema_for::<Condition<i64>>(), false),
            typed("bool", generator.subschema_for::<Condition<bool>>(), false),
            typed(
                "text",
                generator.subschema_for::<Condition<String>>(),
                false,
            ),
            typed("value", generator.subschema_for::<Condition<Value>>(), true),
            typed("edge", generator.subschema_for::<Condition<Edge>>(), false),
        ];

        json_schema!({
            "description": "The points of a member whose value satisfies a condition, over \
                            one of the value types of the format; a path reads a field of \
                            record values",
            "type": "object",
            "properties": {
                "key": key,
                "path": {"type": "string"},
                "type": {"enum": ["f64", "i64", "bool", "text", "value", "edge"]},
                "condition": true
            },
            "required": ["key", "type", "condition"],
            "oneOf": variants
        })
    }
}

impl JsonSchema for Processor {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Processor")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let key = generator.subschema_for::<Key>();
        let output = generator.subschema_for::<Output>();
        let input = generator.subschema_for::<Input>();

        json_schema!({
            "description": "A processor: one recipe, the key of the output it produces, \
                            the formula that produces it, and optionally what it \
                            expects of the inputs it reads",
            "type": "object",
            "properties": {
                "format": {"const": 1},
                "output": key,
                "inputs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"key": key, "input": input},
                        "required": ["key", "input"]
                    }
                },
                "formula": output
            },
            "required": ["format", "output", "formula"]
        })
    }
}

/// The JSON Schema (draft 2020-12) of a processor document, as written by
/// `serde_json`; see the [module documentation](super).
#[must_use]
pub fn processor_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Processor)).expect("a schema is plain JSON")
}
