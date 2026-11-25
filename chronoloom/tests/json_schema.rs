//! The JSON Schema of the processor document, behind the `json-schema` feature:
//! a fixture that fails on any change, and refusals that show the schema is not
//! vacuous. That the schema accepts every document the round-trip tests write is
//! checked in `serialisation.rs`, next to the generators.
#![cfg(feature = "json-schema")]

#[path = "common/schema_validator.rs"]
mod validator;

use serde_json::{json, Value as Json};

use chronoloom::serialisation::processor_json_schema;
use chronoloom::{Condition, Formula, Group, Input, Kind, Pattern, Processor, Schema, Value};

const FIXTURE: &str = "tests/fixtures/processor.schema.json";

/// The schema without its descriptions, which come from the documentation:
/// rewording a comment must not look like a change of the format.
fn without_prose(schema: &Json) -> Json {
    match schema {
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .filter(|(name, value)| {
                    !(matches!(name.as_str(), "description" | "title") && value.is_string())
                })
                .map(|(name, value)| (name.clone(), without_prose(value)))
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(without_prose).collect()),
        other => other.clone(),
    }
}

#[test]
fn the_schema_matches_its_fixture() {
    let schema = without_prose(&processor_json_schema());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);

    if std::env::var_os("CHRONOLOOM_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
        std::fs::write(&path, text).unwrap();
        return;
    }

    let stored = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{FIXTURE} is missing; see the serialisation module"));
    let stored: Json = serde_json::from_str(&stored).unwrap();
    assert_eq!(
        schema, stored,
        "the processor schema changed. Decide whether the format version must change, then \
         run the tests again with CHRONOLOOM_UPDATE_SCHEMA=1 to rewrite {FIXTURE}"
    );
}

#[test]
fn the_schema_is_the_same_every_time() {
    assert_eq!(processor_json_schema(), processor_json_schema());
}

fn problems(document: &Json) -> Vec<String> {
    validator::validate(&processor_json_schema(), document)
}

fn written(processor: &Processor) -> Json {
    serde_json::to_value(processor).unwrap()
}

#[test]
fn the_schema_accepts_a_document_with_declared_inputs() {
    let presence = Processor::new(
        "presence",
        chronoloom::Recipe::per_entity(Group::new("records", "agent"), Formula::state("timeline")),
    )
    .with_input("timeline", Input::Timeline)
    .with_input("edges", Input::Edges)
    .with_input(
        "records",
        Input::Records(Schema::record([
            ("agent", Schema::Text),
            ("dwell", Schema::optional(Schema::Float)),
        ])),
    );
    let busy = Processor::new(
        "busy",
        Formula::while_matching(Pattern::new("floats", Condition::Greater(5.0))),
    )
    .with_input("floats", Input::Points(Kind::Float));

    for processor in [presence, busy] {
        let document = written(&processor);
        assert!(document["inputs"].is_array());
        assert_eq!(problems(&document), Vec::<String>::new());
    }

    let empty = written(&Processor::new("a", Formula::state("b")));
    assert!(empty.get("inputs").is_none());
    assert_eq!(problems(&empty), Vec::<String>::new());
}

#[test]
fn the_schema_accepts_every_shape_of_value_in_a_condition() {
    for constant in [
        Value::Null,
        Value::Bool(true),
        Value::Integer(5),
        Value::Float(5.5),
        Value::from("text"),
        Value::record([("inner", Value::record([("n", Value::Integer(-1))]))]),
    ] {
        let processor = Processor::new(
            "a",
            Formula::occurs(Pattern::field("r", "path", Condition::Equals(constant))),
        );
        assert_eq!(problems(&written(&processor)), Vec::<String>::new());
    }
}

#[test]
fn the_schema_refuses_what_a_reader_refuses() {
    let output = |formula: Json| json!({"format": 1, "output": "a", "formula": formula});
    let pattern = |extra: Json| {
        let mut base = json!({"key": "k", "type": "i64", "condition": {"equals": 1}});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        output(json!({"state": {"occurs": base}}))
    };

    assert_eq!(problems(&pattern(json!({}))), Vec::<String>::new());

    let refused = [
        json!({"format": 2, "output": "a", "formula": {"state": {"state": "b"}}}),
        json!({"output": "a", "formula": {"state": {"state": "b"}}}),
        json!({"format": 1, "output": "a"}),
        json!({"format": 1, "formula": {"state": {"state": "b"}}}),
        json!({"format": 1, "output": 5, "formula": {"state": {"state": "a"}}}),
        json!({"format": 1, "output": "a", "inputs": [{"key": "b", "input": "points"}],
               "formula": {"state": {"state": "b"}}}),
        json!({"format": 1, "output": "a", "inputs": [{"key": "b", "input": {"points": "edge"}}],
               "formula": {"state": {"state": "b"}}}),
        output(json!({"state": {"nonsense": 1}})),
        pattern(json!({"type": "char"})),
        pattern(json!({"path": "a.b"})),
        pattern(json!({"condition": {"equals": "one"}})),
        pattern(json!({"condition": {"sideways": 1}})),
        pattern(json!({"key": 7})),
        output(json!({"state": {"while": {
            "pattern": {"key": "k", "type": "i64", "condition": "any"},
            "timeout": "soon"
        }}})),
        output(json!({"events": {"project": {"key": "a", "path": "p", "into": "complex"}}})),
    ];
    for document in &refused {
        assert!(
            !problems(document).is_empty(),
            "the schema accepts {document}"
        );
    }

    // And the same documents that are valid but for the one change.
    let valid_projection =
        output(json!({"events": {"project": {"key": "a", "path": "p", "into": "integer"}}}));
    assert_eq!(problems(&valid_projection), Vec::<String>::new());
}

/// The lines of the first code block after `heading`, without the fences.
fn first_block(lines: impl Iterator<Item = String>, heading: &str, fence: &str) -> Vec<String> {
    lines
        .skip_while(|line| line != heading)
        .skip_while(|line| line != fence)
        .skip(1)
        .take_while(|line| line != "```")
        .collect()
}

#[test]
fn the_readme_example_is_the_doctest_of_the_serialisation_module() {
    let readme = include_str!("../README.md").lines().map(str::to_owned);
    let module = include_str!("../src/serialisation/mod.rs")
        .lines()
        .map(|line| line.strip_prefix("//!").unwrap_or(line))
        .map(|line| line.strip_prefix(' ').unwrap_or(line).to_owned());

    let documented = first_block(readme, "### A JSON Schema", "```rust,ignore");
    // The lines of the doctest that only gate it on the feature are hidden.
    let tested: Vec<String> = first_block(module, "# JSON Schema", "```")
        .into_iter()
        .filter(|line| !line.starts_with("# "))
        .collect();

    assert!(!documented.is_empty());
    assert_eq!(documented, tested);
}
