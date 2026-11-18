//! A small validator for the subset of JSON Schema the crate emits: `$ref` to
//! `$defs`, `type`, `const`, `enum`, `properties`, `required`,
//! `additionalProperties`, `items`, `oneOf`, `anyOf`, `allOf` and `not`.
//!
//! A keyword it does not know panics, so a schema that starts using a new one
//! is never checked only as far as this validator goes.

use serde_json::Value as Json;

/// Every keyword the validator understands or knowingly ignores.
const KNOWN_KEYWORDS: &[&str] = &[
    "$schema",
    "$defs",
    "$ref",
    "title",
    "description",
    "default",
    "type",
    "format",
    "minimum",
    "const",
    "enum",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "oneOf",
    "anyOf",
    "allOf",
    "not",
];

/// The problems of `instance` against the schema `root`, empty when it is valid.
pub fn validate(root: &Json, instance: &Json) -> Vec<String> {
    let mut problems = Vec::new();
    check(root, root, instance, "$", &mut problems);
    problems
}

fn type_matches(name: &str, instance: &Json) -> bool {
    match name {
        "null" => instance.is_null(),
        "boolean" => instance.is_boolean(),
        "string" => instance.is_string(),
        "array" => instance.is_array(),
        "object" => instance.is_object(),
        "number" => instance.is_number(),
        "integer" => {
            instance.is_i64()
                || instance.is_u64()
                || instance
                    .as_f64()
                    .is_some_and(|number| number.fract() == 0.0)
        }
        other => panic!("the validator does not know the type `{other}`"),
    }
}

fn check(root: &Json, schema: &Json, instance: &Json, at: &str, problems: &mut Vec<String>) {
    let object = match schema {
        Json::Bool(true) => return,
        Json::Bool(false) => {
            problems.push(format!("{at}: nothing is allowed here"));
            return;
        }
        Json::Object(object) => object,
        other => panic!("not a schema: {other}"),
    };

    for keyword in object.keys() {
        assert!(
            KNOWN_KEYWORDS.contains(&keyword.as_str()),
            "{at}: the validator does not know the keyword `{keyword}`"
        );
    }

    if let Some(reference) = object.get("$ref").and_then(Json::as_str) {
        let target = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .unwrap_or_else(|| panic!("the reference `{reference}` does not resolve"));
        check(root, target, instance, at, problems);
    }

    if let Some(types) = object.get("type") {
        let names: Vec<&str> = match types {
            Json::String(name) => vec![name],
            Json::Array(names) => names.iter().filter_map(Json::as_str).collect(),
            other => panic!("not a type: {other}"),
        };
        if !names.iter().any(|name| type_matches(name, instance)) {
            problems.push(format!("{at}: expected {names:?}, found {instance}"));
            return;
        }
    }
    if let Some(constant) = object.get("const") {
        if constant != instance {
            problems.push(format!(
                "{at}: expected the constant {constant}, found {instance}"
            ));
        }
    }
    if let Some(Json::Array(options)) = object.get("enum") {
        if !options.contains(instance) {
            problems.push(format!("{at}: {instance} is none of {options:?}"));
        }
    }

    if let Json::Object(fields) = instance {
        if let Some(Json::Array(required)) = object.get("required") {
            for name in required.iter().filter_map(Json::as_str) {
                if !fields.contains_key(name) {
                    problems.push(format!("{at}: the field `{name}` is required"));
                }
            }
        }
        let properties = object.get("properties").and_then(Json::as_object);
        for (name, value) in fields {
            let path = format!("{at}.{name}");
            let property = properties.and_then(|properties| properties.get(name));
            if let Some(property) = property.or_else(|| object.get("additionalProperties")) {
                check(root, property, value, &path, problems);
            }
        }
    }
    if let (Json::Array(items), Some(each)) = (instance, object.get("items")) {
        for (index, item) in items.iter().enumerate() {
            check(root, each, item, &format!("{at}[{index}]"), problems);
        }
    }

    let valid = |schema: &Json| {
        let mut inner = Vec::new();
        check(root, schema, instance, at, &mut inner);
        inner.is_empty()
    };
    if let Some(Json::Array(options)) = object.get("oneOf") {
        let matching = options.iter().filter(|option| valid(option)).count();
        if matching != 1 {
            problems.push(format!(
                "{at}: {matching} of the oneOf options match {instance}"
            ));
        }
    }
    if let Some(Json::Array(options)) = object.get("anyOf") {
        if !options.iter().any(valid) {
            problems.push(format!("{at}: none of the anyOf options match {instance}"));
        }
    }
    if let Some(Json::Array(parts)) = object.get("allOf") {
        for part in parts {
            check(root, part, instance, at, problems);
        }
    }
    if let Some(forbidden) = object.get("not") {
        if valid(forbidden) {
            problems.push(format!("{at}: {instance} matches what `not` forbids"));
        }
    }
}
