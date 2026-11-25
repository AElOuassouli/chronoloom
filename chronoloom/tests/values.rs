//! Record values and their schema: how values compare and print, how paths
//! reach into records, and what a schema accepts.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use chronoloom::{Condition, Schema, SchemaError, Value};

fn agent(identifier: i64) -> Value {
    Value::record([("id", Value::from(identifier))])
}

fn message() -> Value {
    Value::record([
        ("direction", Value::from("in")),
        ("agent", agent(7)),
        ("dwell", Value::from(1.5)),
    ])
}

// ---------------------------------------------------------------------------
// Ordering and equality
// ---------------------------------------------------------------------------

#[test]
fn values_of_one_kind_compare_naturally() {
    assert_eq!(Value::Null.partial_cmp(&Value::Null), Some(Ordering::Equal));
    assert!(Value::from(false) < Value::from(true));
    assert!(Value::from(-3) < Value::from(2));
    assert!(Value::from(1.5) < Value::from(2.5));
    assert!(Value::from("apple") < Value::from("banana"));
    assert!(Value::from("b") > Value::from("a"));
    assert_eq!(Value::from("a"), Value::from(String::from("a")));
}

#[test]
fn integers_and_floats_compare_as_numbers() {
    assert!(Value::from(5) < Value::from(5.5));
    assert!(Value::from(6) > Value::from(5.5));
    assert!(Value::from(-6) < Value::from(-5.5));
    assert!(Value::from(-5) > Value::from(-5.5));
    assert!(Value::from(5.5) > Value::from(5));
    assert_eq!(Value::from(5), Value::from(5.0));
    assert_eq!(Value::from(5.0), Value::from(5));
    assert_eq!(Value::from(-0.0), Value::from(0));
}

#[test]
fn integers_beyond_the_precision_of_a_float_compare_exactly() {
    let large = 9_007_199_254_740_993_i64; // 2^53 + 1, not a float
    assert!(Value::from(large) > Value::from(9_007_199_254_740_992.0));
    assert_ne!(Value::from(large), Value::from(9_007_199_254_740_992.0));

    // The ends of the integers against the floats at and beyond them.
    assert!(Value::from(i64::MAX) < Value::from(9_223_372_036_854_775_808.0));
    assert_eq!(
        Value::from(i64::MIN),
        Value::from(-9_223_372_036_854_775_808.0)
    );
    assert!(Value::from(i64::MIN) > Value::from(-1e30));
    assert!(Value::from(i64::MAX) < Value::from(f64::INFINITY));
    assert!(Value::from(i64::MIN) > Value::from(f64::NEG_INFINITY));
}

#[test]
fn a_nan_is_unordered_and_equal_to_nothing() {
    let nan = Value::from(f64::NAN);

    assert_eq!(nan.partial_cmp(&nan), None);
    assert_ne!(nan, nan.clone());
    assert_eq!(nan.partial_cmp(&Value::from(1)), None);
    assert_eq!(Value::from(1).partial_cmp(&nan), None);
}

#[test]
fn values_of_other_kinds_are_unordered() {
    let values = [
        Value::Null,
        Value::from(true),
        Value::from(1),
        Value::from("1"),
        agent(1),
    ];

    for (index, first) in values.iter().enumerate() {
        for second in &values[index + 1..] {
            assert_eq!(first.partial_cmp(second), None, "{first} against {second}");
            assert_ne!(first, second);
        }
    }
    assert_eq!(Value::from(1).partial_cmp(&Value::from("1")), None);
}

#[test]
fn records_are_equal_field_by_field_and_otherwise_unordered() {
    assert_eq!(agent(1), agent(1));
    assert_eq!(agent(1).partial_cmp(&agent(1)), Some(Ordering::Equal));
    assert_ne!(agent(1), agent(2));
    assert_eq!(agent(1).partial_cmp(&agent(2)), None);
    // Numbers inside a record compare as numbers too.
    assert_eq!(
        Value::record([("id", Value::from(1.0))]),
        Value::record([("id", Value::from(1))])
    );
    assert_ne!(
        Value::record([("id", Value::from(1))]),
        Value::record([("name", Value::from(1))])
    );
}

// ---------------------------------------------------------------------------
// Paths and accessors
// ---------------------------------------------------------------------------

#[test]
fn get_reaches_nested_fields_by_a_dotted_path() {
    let message = message();

    assert_eq!(message.get("direction"), Some(&Value::from("in")));
    assert_eq!(message.get("agent.id"), Some(&Value::from(7)));
    assert_eq!(message.get("agent"), Some(&agent(7)));
    // The empty path is the value itself.
    assert_eq!(message.get(""), Some(&message));
    assert_eq!(Value::from(3).get(""), Some(&Value::from(3)));
}

#[test]
fn get_finds_nothing_for_a_missing_field_or_a_path_through_a_non_record() {
    let message = message();

    assert_eq!(message.get("speed"), None);
    assert_eq!(message.get("agent.name"), None);
    assert_eq!(message.get("agent.id.digit"), None);
    assert_eq!(message.get("direction.first"), None);
    assert_eq!(Value::from(3).get("x"), None);
    // A segment is a name taken literally: no trailing dot, no empty segment.
    assert_eq!(message.get("agent."), None);
    assert_eq!(message.get(".agent"), None);
    assert_eq!(message.get("agent..id"), None);
}

#[test]
fn an_empty_segment_names_the_field_with_an_empty_name() {
    let odd = Value::record([("", Value::from(1)), ("a.b", Value::from(2))]);

    assert_eq!(odd.get("."), None);
    assert_eq!(odd.get("a.b"), None, "a name with a dot cannot be reached");
    assert_eq!(
        Value::record([("", agent(1))]).get(".id"),
        Some(&Value::from(1))
    );
}

#[test]
fn typed_accessors_read_their_kind_only() {
    assert_eq!(Value::from(true).as_bool(), Some(true));
    assert_eq!(Value::from(1).as_bool(), None);

    assert_eq!(Value::from(7).as_integer(), Some(7));
    assert_eq!(Value::from(7.0).as_integer(), None);

    assert_eq!(Value::from(1.5).as_float(), Some(1.5));
    assert_eq!(
        Value::from(7).as_float(),
        Some(7.0),
        "an integer reads as a float"
    );
    assert_eq!(Value::from("7").as_float(), None);

    assert_eq!(Value::from("in").as_text(), Some("in"));
    assert_eq!(Value::from(1).as_text(), None);

    assert_eq!(
        agent(7).as_record().map(BTreeMap::len),
        Some(1),
        "a record reads as its fields"
    );
    assert_eq!(Value::Null.as_record(), None);

    let names: Vec<&str> = [
        Value::Null,
        Value::from(true),
        Value::from(1),
        Value::from(1.0),
        Value::from("a"),
        agent(1),
    ]
    .iter()
    .map(Value::kind_name)
    .collect();
    assert_eq!(
        names,
        ["null", "bool", "integer", "float", "text", "record"]
    );
}

// ---------------------------------------------------------------------------
// Construction and display
// ---------------------------------------------------------------------------

#[test]
fn from_conversions_build_each_kind() {
    assert_eq!(Value::from(true), Value::Bool(true));
    assert_eq!(Value::from(4_i64), Value::Integer(4));
    assert_eq!(Value::from(0.5_f64), Value::Float(0.5));
    assert_eq!(Value::from("text"), Value::Text(String::from("text")));
    assert_eq!(
        Value::from(String::from("text")),
        Value::Text(String::from("text"))
    );

    let fields = BTreeMap::from([(String::from("id"), Value::from(1))]);
    assert_eq!(Value::from(fields), agent(1));
}

#[test]
fn a_record_given_a_name_twice_keeps_the_last() {
    let record = Value::record([("n", Value::from(1)), ("n", Value::from(2))]);

    assert_eq!(record.get("n"), Some(&Value::from(2)));
}

#[test]
fn display_writes_json_like_text() {
    assert_eq!(Value::Null.to_string(), "null");
    assert_eq!(Value::from(true).to_string(), "true");
    assert_eq!(Value::from(-7).to_string(), "-7");
    assert_eq!(Value::from(1.5).to_string(), "1.5");
    assert_eq!(Value::from("in").to_string(), r#""in""#);
    assert_eq!(
        Value::record(Vec::<(String, Value)>::new()).to_string(),
        "{}"
    );
    assert_eq!(
        message().to_string(),
        r#"{"agent": {"id": 7}, "direction": "in", "dwell": 1.5}"#
    );
}

#[test]
fn display_keeps_a_float_apart_from_an_integer() {
    assert_eq!(Value::from(5).to_string(), "5");
    assert_eq!(Value::from(5.0).to_string(), "5.0");
    assert_eq!(Value::from(f64::NAN).to_string(), "NaN");
    assert_eq!(Value::from(f64::INFINITY).to_string(), "inf");
    assert_eq!(Value::from(f64::NEG_INFINITY).to_string(), "-inf");
}

#[test]
fn display_escapes_text_as_json_does() {
    assert_eq!(
        Value::from("say \"hi\"\\\n\t\r").to_string(),
        r#""say \"hi\"\\\n\t\r""#
    );
    assert_eq!(Value::from("\u{1}").to_string(), r#""\u0001""#);
    assert_eq!(Value::from("é").to_string(), "\"é\"");
    assert_eq!(
        Value::record([("a\"b", Value::Null)]).to_string(),
        r#"{"a\"b": null}"#
    );
}

// ---------------------------------------------------------------------------
// Conditions over values
// ---------------------------------------------------------------------------

#[test]
fn an_ordering_condition_matches_numbers_of_either_kind_only() {
    let above = Condition::Greater(Value::from(5));

    assert!(above.matches(&Value::from(6)));
    assert!(above.matches(&Value::from(5.5)));
    assert!(!above.matches(&Value::from(5)));
    assert!(!above.matches(&Value::from(5.0)));
    assert!(!above.matches(&Value::from(4.9)));
    // Another kind is unordered with a number, so no ordering condition matches.
    assert!(!above.matches(&Value::from("9")));
    assert!(!above.matches(&Value::Null));
    assert!(!above.matches(&agent(9)));
    assert!(!above.matches(&Value::from(f64::NAN)));

    let outside = Condition::Not(Box::new(above));
    assert!(
        outside.matches(&Value::from("9")),
        "not negates a failed comparison"
    );
}

#[test]
fn conditions_compose_over_values() {
    let moderate = Condition::Within {
        low: Value::from(5),
        high: Value::from(10.0),
    };

    assert!(moderate.matches(&Value::from(5.0)));
    assert!(moderate.matches(&Value::from(9)));
    assert!(!moderate.matches(&Value::from(10)));
    assert!(Condition::Equals(Value::from(5)).matches(&Value::from(5.0)));
    assert!(Condition::In(vec![Value::from("a"), Value::from(2)]).matches(&Value::from(2.0)));
    assert!(Condition::Equals(agent(1)).matches(&agent(1)));
    assert_eq!(Condition::Greater(Value::from(5)).to_string(), "value > 5");
    assert_eq!(
        Condition::Equals(Value::from("start")).to_string(),
        r#"value = "start""#
    );
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

fn error(schema: &Schema, value: &Value) -> SchemaError {
    schema.check(value).expect_err("the value is refused")
}

fn at(path: &str, expected: &str, found: &str) -> SchemaError {
    SchemaError {
        path: path.to_owned(),
        expected: expected.to_owned(),
        found: found.to_owned(),
    }
}

#[test]
fn each_scalar_schema_accepts_its_kind_and_names_the_mismatch() {
    let cases = [
        (Schema::Null, Value::Null, "null"),
        (Schema::Bool, Value::from(true), "bool"),
        (Schema::Integer, Value::from(3), "integer"),
        (Schema::Float, Value::from(3.5), "float"),
        (Schema::Text, Value::from("a"), "text"),
        (
            Schema::record([("id", Schema::Integer)]),
            agent(1),
            "record",
        ),
    ];
    let all: Vec<Value> = cases.iter().map(|(_, value, _)| value.clone()).collect();

    for (schema, accepted, name) in &cases {
        assert_eq!(schema.check(accepted), Ok(()));
        for value in all.iter().filter(|value| *value != accepted) {
            // An integer is accepted as a float.
            if matches!(schema, Schema::Float) && matches!(value, Value::Integer(_)) {
                continue;
            }
            assert_eq!(
                error(schema, value),
                at("", name, value.kind_name()),
                "{schema:?} against {value}"
            );
        }
    }
}

#[test]
fn any_accepts_every_value() {
    for value in [Value::Null, Value::from(1), Value::from("a"), agent(1)] {
        assert_eq!(Schema::Any.check(&value), Ok(()));
    }
}

#[test]
fn a_float_schema_accepts_an_integer_and_an_integer_schema_refuses_a_float() {
    assert_eq!(Schema::Float.check(&Value::from(3)), Ok(()));
    assert_eq!(
        error(&Schema::Integer, &Value::from(3.0)),
        at("", "integer", "float")
    );
}

#[test]
fn optional_accepts_null_and_the_shape_and_may_be_left_out_of_a_record() {
    let optional = Schema::optional(Schema::Integer);

    assert_eq!(optional.check(&Value::Null), Ok(()));
    assert_eq!(optional.check(&Value::from(2)), Ok(()));
    assert_eq!(
        error(&optional, &Value::from("2")),
        at("", "optional integer", "text")
    );
    assert_eq!(
        Schema::optional(Schema::optional(Schema::Text)).check(&Value::Null),
        Ok(())
    );

    let record = Schema::record([("id", Schema::Integer), ("age", optional)]);
    assert_eq!(record.check(&agent(1)), Ok(()), "left out");
    assert_eq!(
        record.check(&Value::record([
            ("id", Value::from(1)),
            ("age", Value::Null)
        ])),
        Ok(())
    );
    assert_eq!(
        record.check(&Value::record([
            ("id", Value::from(1)),
            ("age", Value::from(30))
        ])),
        Ok(())
    );
}

#[test]
fn a_required_field_that_is_absent_or_null_is_refused() {
    let record = Schema::record([("id", Schema::Integer)]);

    assert_eq!(
        error(&record, &Value::record(Vec::<(String, Value)>::new())),
        at("id", "integer", "missing")
    );
    assert_eq!(
        error(&record, &Value::record([("id", Value::Null)])),
        at("id", "integer", "null")
    );
    assert_eq!(error(&record, &Value::from(1)), at("", "record", "integer"));
}

#[test]
fn a_record_rejects_unknown_fields() {
    let record = Schema::record([("id", Schema::Integer)]);
    let extra = Value::record([("id", Value::from(1)), ("name", Value::from("x"))]);

    assert_eq!(error(&record, &extra), at("name", "no such field", "text"));
    assert_eq!(
        error(&Schema::record(Vec::<(String, Schema)>::new()), &agent(1)),
        at("id", "no such field", "integer")
    );
}

#[test]
fn nested_records_report_the_dotted_path_of_the_first_offence() {
    let schema = Schema::record([
        (
            "agent",
            Schema::record([("id", Schema::Integer), ("zone", Schema::Text)]),
        ),
        ("direction", Schema::Text),
    ]);
    let wrong = Value::record([
        (
            "agent",
            Value::record([("id", Value::from("7")), ("zone", Value::from(3))]),
        ),
        ("direction", Value::from(1)),
    ]);

    // In field order, depth first: `agent.id` comes before `agent.zone` and
    // before `direction`.
    assert_eq!(error(&schema, &wrong), at("agent.id", "integer", "text"));

    let missing = Value::record([
        ("agent", Value::record([("id", Value::from(7))])),
        ("direction", Value::from("in")),
    ]);
    assert_eq!(
        error(&schema, &missing),
        at("agent.zone", "text", "missing")
    );

    let unknown = Value::record([
        (
            "agent",
            Value::record([
                ("id", Value::from(7)),
                ("zone", Value::from("a")),
                ("rank", Value::from(1)),
            ]),
        ),
        ("direction", Value::from("in")),
    ]);
    assert_eq!(
        error(&schema, &unknown),
        at("agent.rank", "no such field", "integer")
    );
}

#[test]
fn an_optional_record_may_be_null_or_checked_inside() {
    let schema = Schema::record([(
        "agent",
        Schema::optional(Schema::record([("id", Schema::Integer)])),
    )]);

    assert_eq!(
        schema.check(&Value::record(Vec::<(String, Value)>::new())),
        Ok(())
    );
    assert_eq!(
        schema.check(&Value::record([("agent", Value::Null)])),
        Ok(())
    );
    assert_eq!(schema.check(&Value::record([("agent", agent(1))])), Ok(()));
    assert_eq!(
        error(
            &schema,
            &Value::record([("agent", Value::record([("id", Value::from(1.5))]))])
        ),
        at("agent.id", "integer", "float")
    );
}

#[test]
fn a_schema_error_reads_as_a_sentence() {
    assert_eq!(
        at("agent.id", "integer", "text").to_string(),
        "at `agent.id`: expected integer, found text"
    );
    assert_eq!(
        at("", "record", "integer").to_string(),
        "at `the value`: expected record, found integer"
    );
}

#[test]
fn schemas_compare_and_clone() {
    let schema = Schema::record([("id", Schema::Integer)]);

    assert_eq!(schema, schema.clone());
    assert_ne!(schema, Schema::record([("id", Schema::Float)]));
    assert_ne!(Schema::Integer, Schema::optional(Schema::Integer));
}
