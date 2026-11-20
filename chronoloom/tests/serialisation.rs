//! Saving and loading, behind the `serde` feature: lossless round trips of
//! sequences, systems and processors, and the refusals of a document that would
//! break an invariant.
#![cfg(feature = "serde")]

#[path = "common/random.rs"]
mod common;
#[cfg(feature = "json-schema")]
#[path = "common/schema_validator.rs"]
mod validator;

use serde_json::{json, Value as Json};

use chronoloom::serialisation::FORMAT_VERSION;
use chronoloom::stream_system::StreamSystem;
use chronoloom::{
    Condition, Edge, Extent, Formula, Group, Input, Kind, OpenEnd, Pattern, Processor, Recipe,
    Schema, Sequence, SequenceSystem, TimeIntervalEvent, TimeIntervalSequence, TimePointEvent,
    TimePointSequence, Value,
};

use common::{bounds, extent, for_each_seed, span, spans, Random, DOMAIN_END, DOMAIN_START};

const CASES: u64 = 300;

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    let text = serde_json::to_string(value).expect("the value serialises");
    serde_json::from_str(&text).expect("the document loads")
}

fn load<T: serde::de::DeserializeOwned>(document: &Json) -> Result<T, String> {
    serde_json::from_value(document.clone()).map_err(|error| error.to_string())
}

fn refusal<T: serde::de::DeserializeOwned>(document: &Json) -> String {
    match load::<T>(document) {
        Ok(_) => panic!("the document should have been refused: {document}"),
        Err(message) => message,
    }
}

// ---------------------------------------------------------------- values

#[test]
fn a_value_is_plain_json() {
    let value = Value::record([
        ("none", Value::Null),
        ("flag", Value::Bool(true)),
        ("count", Value::Integer(5)),
        ("ratio", Value::Float(5.5)),
        ("name", Value::from("text")),
        ("inner", Value::record([("n", Value::Integer(-1))])),
    ]);

    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        json!({
            "none": null, "flag": true, "count": 5, "ratio": 5.5, "name": "text",
            "inner": {"n": -1}
        })
    );
    assert_eq!(round_trip(&value), value);
}

#[test]
fn an_integer_and_a_float_each_come_back_as_their_own_variant() {
    let integer = round_trip(&Value::Integer(5));
    let float = round_trip(&Value::Float(5.0));

    assert!(matches!(integer, Value::Integer(5)), "{integer:?}");
    assert!(
        matches!(float, Value::Float(number) if number == 5.0),
        "{float:?}"
    );
    assert_eq!(serde_json::to_string(&Value::Integer(5)).unwrap(), "5");
    assert_eq!(serde_json::to_string(&Value::Float(5.0)).unwrap(), "5.0");
    assert_eq!(Value::Integer(5), Value::Float(5.0));

    for number in [i64::MIN, i64::MAX, 0, -7] {
        assert!(
            matches!(round_trip(&Value::Integer(number)), Value::Integer(back) if back == number)
        );
    }
    for number in [1e300, -0.5, 1e16, 123_456_789.0] {
        assert!(matches!(round_trip(&Value::Float(number)), Value::Float(back) if back == number));
    }
}

#[test]
fn a_value_refuses_what_it_cannot_hold() {
    assert!(serde_json::to_string(&Value::Float(f64::NAN)).is_err());
    assert!(serde_json::to_string(&Value::Float(f64::INFINITY)).is_err());
    assert!(serde_json::from_str::<Value>("18446744073709551615").is_err());
    assert!(serde_json::from_str::<Value>("[1, 2]").is_err());
}

// ------------------------------------------------------------- sequences

#[test]
fn random_timelines_round_trip_losslessly() {
    for_each_seed(CASES, |random, _| {
        let mut timeline =
            TimeIntervalSequence::from_spans("up", spans(random, 6)).with_observed(extent(random));
        match random.below(3) {
            0 => timeline = timeline.transform(-2, 3).unwrap(),
            1 => timeline = timeline.complement(),
            _ => {}
        }

        let loaded = round_trip(&timeline);

        assert_eq!(loaded, timeline);
        assert_eq!(bounds(&loaded), bounds(&timeline));
        assert_eq!(loaded.observed(), timeline.observed());
        assert_eq!(loaded.transformation(), timeline.transformation());
        assert_eq!(loaded.attribute(), timeline.attribute());
        assert_eq!(loaded.active_duration(), timeline.active_duration());
    });
}

#[test]
fn random_point_sequences_round_trip_losslessly() {
    for_each_seed(CASES, |random, _| {
        let events = (0..random.below(8))
            .map(|order| {
                TimePointEvent::new(random.between(DOMAIN_START, DOMAIN_END), order as i64)
            })
            .collect();
        let points =
            TimePointSequence::from_events("readings", events).with_observed(extent(random));

        assert_eq!(round_trip(&points), points);
    });
}

#[test]
fn a_timeline_document_has_a_stable_shape() {
    let up = TimeIntervalSequence::from_spans("up", vec![span(0, 10), span(20, 30)])
        .with_observed(Extent::from_spans(vec![span(0, 40)]));

    assert_eq!(
        serde_json::to_value(&up).unwrap(),
        json!({
            "format": 1,
            "attribute": {"name": "up"},
            "transformation": [0, 0],
            "observed": [[0, 40]],
            "spans": [[0, 10], [20, 30]]
        })
    );
    assert_eq!(FORMAT_VERSION, 1);
}

#[test]
fn spans_and_extents_that_are_not_normalized_are_normalized() {
    let timeline: TimeIntervalSequence = load(&json!({
        "format": 1,
        "attribute": {"name": "up"},
        "observed": [[20, 40], [0, 25]],
        "spans": [[20, 30], [0, 5], [5, 9], [7, 12]]
    }))
    .unwrap();

    assert_eq!(bounds(&timeline), [(0, 12), (20, 30)]);
    assert_eq!(timeline.observed().as_slice().len(), 1);
    assert_eq!(timeline.observed().as_slice()[0].bounds(), (0, 40));
}

#[test]
fn events_are_sorted_keeping_the_order_of_ties() {
    let points: TimePointSequence<i64> = load(&json!({
        "format": 1,
        "attribute": {"name": "readings"},
        "events": [
            {"timestamp": 9, "value": 0},
            {"timestamp": 3, "value": 1},
            {"timestamp": 9, "value": 2},
            {"timestamp": 3, "value": 3}
        ]
    }))
    .unwrap();

    let order: Vec<(i64, i64)> = points
        .iter()
        .map(|event| (event.timestamp(), *event.value()))
        .collect();
    assert_eq!(order, [(3, 1), (3, 3), (9, 0), (9, 2)]);
    assert!(
        points.observed().contains(0),
        "an omitted extent is everything"
    );
}

#[test]
fn a_point_at_the_end_of_time_is_kept_outside_every_extent() {
    let points: TimePointSequence<i64> = load(&json!({
        "format": 1,
        "attribute": {"name": "readings"},
        "observed": [[0, 10]],
        "events": [{"timestamp": i64::MAX, "value": 1}]
    }))
    .unwrap();

    assert_eq!(points.len(), 1);
}

#[test]
fn sequences_refuse_data_outside_their_extent() {
    let timeline = refusal::<TimeIntervalSequence>(&json!({
        "format": 1, "attribute": {"name": "up"}, "observed": [[0, 10]], "spans": [[5, 12]]
    }));
    let points = refusal::<TimePointSequence<i64>>(&json!({
        "format": 1, "attribute": {"name": "p"}, "observed": [[0, 10]],
        "events": [{"timestamp": 10, "value": 1}]
    }));

    assert!(timeline.contains("[5, 12)"), "{timeline}");
    assert!(
        points.contains("at 10") || points.contains("event at 10"),
        "{points}"
    );
}

#[test]
fn a_span_that_ends_before_it_starts_is_refused() {
    for pair in [json!([5, 5]), json!([9, 2])] {
        let message = refusal::<TimeIntervalSequence>(&json!({
            "format": 1, "attribute": {"name": "up"}, "spans": [pair]
        }));
        assert!(message.contains("strictly after"), "{message}");
    }
    assert!(load::<Extent>(&json!([[4, 4]])).is_err());

    let event = refusal::<TimeIntervalEvent<String>>(&json!({"start": 3, "end": 1, "value": "x"}));
    assert!(event.contains("strictly after"), "{event}");
    let valid: TimeIntervalEvent<String> =
        load(&json!({"start": 1, "end": 3, "value": "x"})).unwrap();
    assert_eq!(round_trip(&valid), valid);
}

#[test]
fn every_document_refuses_another_format_version() {
    let documents = [
        (
            json!({"format": 2, "attribute": {"name": "up"}, "spans": []}),
            "interval",
        ),
        (
            json!({"format": 0, "attribute": {"name": "p"}, "events": []}),
            "point",
        ),
        (json!({"format": 7, "members": []}), "system"),
        (json!({"format": 2, "outputs": []}), "processor"),
    ];

    for (document, kind) in documents {
        let message = match kind {
            "interval" => refusal::<TimeIntervalSequence>(&document),
            "point" => refusal::<TimePointSequence<i64>>(&document),
            "system" => refusal::<SequenceSystem>(&document),
            _ => refusal::<Processor>(&document),
        };
        assert!(
            message.contains("unsupported format version"),
            "{kind}: {message}"
        );
    }

    let missing = refusal::<SequenceSystem>(&json!({"members": []}));
    assert!(missing.contains("format"), "{missing}");
}

// --------------------------------------------------------------- systems

fn metadata_of(random: &mut Random, system: &mut SequenceSystem, key: &str) {
    if random.chance(1, 2) {
        let metadata = system.member_metadata_mut(key).expect("a member");
        metadata.insert("unit".to_owned(), format!("tick{}", random.below(3)));
        metadata.insert("origin".to_owned(), "start of the day".to_owned());
    }
}

fn record(random: &mut Random) -> Value {
    let mut fields = vec![("n", Value::Integer(random.between(0, 3)))];
    if random.chance(2, 3) {
        fields.push(("x", Value::Float(random.between(0, 8) as f64 / 2.0)));
    }
    if random.chance(1, 4) {
        fields.push(("note", Value::from("a note")));
    }

    Value::record(fields)
}

/// A system with a member of every shape and value type, metadata, a schema
/// and live members.
fn full_system(random: &mut Random) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    let at = |random: &mut Random| random.between(DOMAIN_START, DOMAIN_END);

    let count = (0..random.below(6)).map(|_| TimePointEvent::new(at(random), random.between(0, 4)));
    system.insert(
        "count",
        TimePointSequence::from_events("count", count.collect()).with_observed(extent(random)),
    );
    let temperature = (0..random.below(5))
        .map(|_| TimePointEvent::new(at(random), random.between(-20, 20) as f64 / 4.0));
    system.insert(
        "temperature",
        TimePointSequence::from_events("temperature", temperature.collect()),
    );
    let flags = (0..random.below(5)).map(|_| TimePointEvent::new(at(random), random.chance(1, 2)));
    system.insert(
        "flag",
        TimePointSequence::from_events("flag", flags.collect()),
    );
    let lines = (0..random.below(4))
        .map(|_| TimePointEvent::new(at(random), format!("line {}", random.below(9))));
    system.insert(
        "log",
        TimePointSequence::from_events("log", lines.collect()),
    );
    let edges = (0..random.below(4)).map(|_| {
        let edge = if random.chance(1, 2) {
            Edge::Start
        } else {
            Edge::End
        };
        TimePointEvent::new(at(random), edge)
    });
    system.insert(
        "edges",
        TimePointSequence::from_events("edges", edges.collect()),
    );
    system.insert(
        "up",
        TimeIntervalSequence::from_spans("up", spans(random, 5)).with_observed(extent(random)),
    );
    system.insert(
        "shifted",
        TimeIntervalSequence::from_spans("raw", spans(random, 5))
            .transform(-1, 2)
            .unwrap(),
    );

    let records = (0..random.below(7)).map(|_| TimePointEvent::new(at(random), record(random)));
    system.insert(
        "record",
        TimePointSequence::from_events("record", records.collect()),
    );
    if random.chance(2, 3) {
        let schema = Schema::record([
            ("n", Schema::Integer),
            ("x", Schema::optional(Schema::Float)),
            ("note", Schema::optional(Schema::Text)),
        ]);
        system.set_schema("record", schema).unwrap();
    }

    system = with_derived_members(random, &system);

    system.insert_live("live", TimePointSequence::<i64>::new("live"), 10);
    system.insert_live("live up", TimeIntervalSequence::new("live up"), 10);
    for tick in 0..random.below(4) {
        system
            .record_point("live", TimePointEvent::new(10 + tick as i64, tick as i64))
            .unwrap();
    }
    system.record_span("live up", span(12, 40)).unwrap();
    system
        .record_point("live", TimePointEvent::new(30, -1_i64))
        .unwrap();
    system.advance("live", 10 + random.between(0, 12)).unwrap();
    system.advance_unobserved("live up", 14).unwrap();

    system
        .metadata_mut()
        .insert("subject".to_owned(), "machine 7".to_owned());
    let keys: Vec<String> = system.iter().map(|(key, _)| key.to_string()).collect();
    for key in keys {
        metadata_of(random, &mut system, &key);
    }

    system
}

/// `system` with members that carry the output they came from, and the entity
/// of a per-entity output, which only a document can file beside a member.
fn with_derived_members(random: &mut Random, system: &SequenceSystem) -> SequenceSystem {
    let mut derived = SequenceSystem::new();
    derived.insert(
        "busy",
        TimeIntervalSequence::from_spans("busy", spans(random, 4)),
    );
    let entities = 1 + random.below(2) as i64;
    for entity in 0..entities {
        let key = format!("present/{entity}");
        let events = (0..random.below(3))
            .map(|_| TimePointEvent::new(random.between(DOMAIN_START, DOMAIN_END), 1_i64))
            .collect();
        derived.insert(
            key.as_str(),
            TimePointSequence::from_events(key.as_str(), events),
        );
    }

    let mut document = serde_json::to_value(system).unwrap();
    let mut added = serde_json::to_value(&derived).unwrap();
    for member in added["members"].as_array_mut().unwrap() {
        if member["key"] == "busy" {
            member["lineage"] = json!({"state": {"state": "up"}});
        } else {
            member["lineage"] = json!({"per_entity": {
                "by": {"key": "record", "path": "n"}, "formula": {"state": "up"}
            }});
            let entity: i64 = member["key"].as_str().unwrap()["present/".len()..]
                .parse()
                .unwrap();
            member["entity"] = json!(entity);
        }
    }
    document["members"]
        .as_array_mut()
        .unwrap()
        .extend(added["members"].as_array().unwrap().iter().cloned());

    load(&document).expect("the document loads")
}

#[test]
fn random_systems_round_trip_losslessly() {
    for_each_seed(CASES, |random, _| {
        let system = full_system(random);

        let text = serde_json::to_string(&system).unwrap();
        let loaded: SequenceSystem = serde_json::from_str(&text).unwrap();

        assert_eq!(loaded, system);
        assert_eq!(loaded.metadata(), system.metadata());

        // The derived members keep the output and the entity filed beside them.
        let saved: Json = serde_json::from_str(&text).unwrap();
        let members = saved["members"].as_array().unwrap();
        let find = |key: &str| members.iter().find(|member| member["key"] == key).unwrap();
        assert_eq!(find("busy")["lineage"], json!({"state": {"state": "up"}}));
        assert_eq!(find("present/0")["entity"], json!(0));
        assert!(find("present/0")["lineage"].get("per_entity").is_some());
        for (key, _) in &system {
            let key = key.as_str();
            assert_eq!(loaded.frontier_of(key), system.frontier_of(key), "{key}");
            assert_eq!(loaded.schema(key), system.schema(key), "{key}");
            assert_eq!(
                loaded.member_metadata(key),
                system.member_metadata(key),
                "{key}"
            );
        }
        assert_eq!(
            serde_json::to_string(&loaded).unwrap(),
            text,
            "writing is stable"
        );
    });
}

#[test]
fn a_loaded_system_keeps_behaving_like_the_original() {
    let mut random = Random::new(11);
    let mut system = full_system(&mut random);
    let mut loaded = round_trip(&system);

    // A live member refuses late events, a schema still refuses a wrong value.
    let late = TimePointEvent::new(0, 1_i64);
    assert_eq!(
        loaded.record_point("live", late).unwrap_err(),
        system.record_point("live", late).unwrap_err()
    );
    if loaded.schema("record").is_some() {
        let wrong = TimePointEvent::new(3, Value::from("not a record"));
        assert!(loaded.record_point("record", wrong).is_err());
    }
}

#[test]
fn a_system_document_has_a_stable_shape() {
    let mut system = SequenceSystem::new();
    system.insert(
        "count",
        TimePointSequence::from_events("count", vec![TimePointEvent::new(3, 7_i64)]),
    );
    system.insert(
        "up",
        TimeIntervalSequence::from_spans("up", vec![span(0, 5)]),
    );
    system
        .metadata_mut()
        .insert("unit".to_owned(), "seconds".to_owned());
    system.insert_live("live", TimePointSequence::<bool>::new("live"), 4);

    let everything = json!([[i64::MIN, i64::MAX]]);
    assert_eq!(
        serde_json::to_value(&system).unwrap(),
        json!({
            "format": 1,
            "metadata": {"unit": "seconds"},
            "members": [
                {
                    "key": "count",
                    "sequence": {
                        "type": "i64", "attribute": {"name": "count"},
                        "observed": everything, "events": [{"timestamp": 3, "value": 7}]
                    },
                    "metadata": {}
                },
                {
                    "key": "live",
                    "sequence": {
                        "type": "bool", "attribute": {"name": "live"},
                        "observed": [[i64::MIN, 4]], "events": []
                    },
                    "metadata": {},
                    "frontier": 4
                },
                {
                    "key": "up",
                    "sequence": {
                        "type": "intervals", "attribute": {"name": "up"},
                        "transformation": [0, 0], "observed": everything, "spans": [[0, 5]]
                    },
                    "metadata": {}
                }
            ]
        })
    );
}

#[test]
fn a_point_member_of_another_value_type_cannot_be_saved() {
    let mut system = SequenceSystem::new();
    system.insert("letters", TimePointSequence::<char>::new("letters"));

    let error = serde_json::to_string(&system).unwrap_err().to_string();
    assert!(
        error.contains("`letters`") && error.contains("char"),
        "{error}"
    );
}

fn member(key: &str, sequence: Json) -> Json {
    json!({"key": key, "sequence": sequence})
}

fn points(kind: &str, events: Json) -> Json {
    json!({"type": kind, "attribute": {"name": "x"}, "events": events})
}

fn system_of(members: Vec<Json>) -> Json {
    json!({"format": 1, "members": members})
}

#[test]
fn a_member_of_an_unknown_value_type_is_refused_by_key() {
    let message = refusal::<SequenceSystem>(&system_of(vec![
        member("good", points("i64", json!([]))),
        member("odd", points("complex", json!([]))),
    ]));

    assert!(
        message.contains("`odd`") && message.contains("does not support"),
        "{message}"
    );
}

#[test]
fn a_member_whose_data_breaks_its_schema_is_refused_by_key() {
    let mut document = member(
        "records",
        points("value", json!([{"timestamp": 1, "value": {"n": "one"}}])),
    );
    document["schema"] = json!({"record": {"n": "integer"}});

    let message = refusal::<SequenceSystem>(&system_of(vec![document]));
    assert!(
        message.contains("`records`") && message.contains("at `n`"),
        "{message}"
    );

    let mut wrong_type = member("count", points("i64", json!([])));
    wrong_type["schema"] = json!("any");
    assert!(refusal::<SequenceSystem>(&system_of(vec![wrong_type])).contains("`count`"));

    let mut fitting = member(
        "records",
        points("value", json!([{"timestamp": 1, "value": {"n": 1}}])),
    );
    fitting["schema"] = json!({"record": {"n": "integer"}});
    let system: SequenceSystem = load(&system_of(vec![fitting])).unwrap();
    assert!(system.schema("records").is_some());
}

#[test]
fn a_member_with_data_outside_its_extent_is_refused_by_key() {
    let mut outside = member("p", points("i64", json!([{"timestamp": 50, "value": 1}])));
    outside["sequence"]["observed"] = json!([[0, 10]]);

    let message = refusal::<SequenceSystem>(&system_of(vec![outside]));
    assert!(
        message.contains("`p`") && message.contains("outside"),
        "{message}"
    );
}

#[test]
fn a_live_member_holds_data_outside_its_extent_only_from_its_frontier() {
    let mut live = member("p", points("i64", json!([{"timestamp": 12, "value": 1}])));
    live["sequence"]["observed"] = json!([[0, 10]]);
    live["frontier"] = json!(10);
    let system: SequenceSystem = load(&system_of(vec![live.clone()])).unwrap();
    assert_eq!(system.frontier_of("p"), Some(10));
    assert_eq!(system.points::<i64>("p").unwrap().len(), 1);

    // Before its frontier, a live member's data lies inside its extent.
    let mut before = live.clone();
    before["sequence"]["events"] = json!([{"timestamp": 5, "value": 1}]);
    before["sequence"]["observed"] = json!([[0, 3]]);
    let message = refusal::<SequenceSystem>(&system_of(vec![before]));
    assert!(
        message.contains("`p`") && message.contains("outside"),
        "{message}"
    );

    // An extent past the frontier is not what a live member holds.
    let mut past = live;
    past["sequence"]["observed"] = json!([[0, 15]]);
    let message = refusal::<SequenceSystem>(&system_of(vec![past]));
    assert!(
        message.contains("`p`") && message.contains("frontier"),
        "{message}"
    );
}

#[test]
fn what_is_filed_beside_a_member_must_agree_with_it() {
    let lineage = json!({"state": {"state": "up"}});
    let mut derived_and_live = member("d", points("i64", json!([])));
    derived_and_live["frontier"] = json!(3);
    derived_and_live["lineage"] = lineage.clone();
    assert!(refusal::<SequenceSystem>(&system_of(vec![derived_and_live])).contains("`d`"));

    let mut entity_without_lineage = member("out/7", points("i64", json!([])));
    entity_without_lineage["entity"] = json!(7);
    assert!(refusal::<SequenceSystem>(&system_of(vec![entity_without_lineage])).contains("entity"));

    let per_entity = json!({"per_entity": {
        "by": {"key": "gate", "path": "agent"}, "formula": {"state": "up"}
    }});
    let mut misfiled = member("out/8", points("i64", json!([])));
    misfiled["lineage"] = per_entity.clone();
    misfiled["entity"] = json!(7);
    assert!(refusal::<SequenceSystem>(&system_of(vec![misfiled])).contains("`out/8`"));

    let mut filed = member("out/7", points("i64", json!([])));
    filed["lineage"] = per_entity;
    filed["entity"] = json!(7);
    // What is filed beside a member is saved and loaded back unchanged.
    let system: SequenceSystem = load(&system_of(vec![filed])).unwrap();
    assert_eq!(round_trip(&system), system);
    let saved = serde_json::to_value(&system).unwrap();
    assert_eq!(saved["members"][0]["entity"], json!(7));
}

#[test]
fn a_key_that_appears_twice_is_refused() {
    let message = refusal::<SequenceSystem>(&system_of(vec![
        member("p", points("i64", json!([]))),
        member("p", points("bool", json!([]))),
    ]));

    assert!(
        message.contains("`p`") && message.contains("twice"),
        "{message}"
    );
}

// ------------------------------------------------------------ processors

fn condition(random: &mut Random) -> Condition<i64> {
    let bound = random.between(0, 4);
    match random.below(7) {
        0 => Condition::Equals(bound),
        1 => Condition::Less(bound),
        2 => Condition::GreaterOrEqual(bound),
        3 => Condition::Within {
            low: bound,
            high: bound + 2,
        },
        4 => Condition::In(vec![bound, bound + 1]),
        5 => Condition::Not(Box::new(Condition::Equals(bound))),
        _ => Condition::Or(vec![
            Condition::Less(1),
            Condition::And(vec![Condition::Any, Condition::Equals(bound)]),
        ]),
    }
}

fn pattern(random: &mut Random) -> Pattern {
    let key = if random.chance(1, 2) { "p" } else { "q" };
    Pattern::new(key, condition(random))
}

fn leaf(random: &mut Random) -> Formula {
    match random.below(6) {
        0 => Formula::state(if random.chance(1, 2) { "s" } else { "t" }),
        1 => Formula::occurs(pattern(random)),
        2 => Formula::while_matching(pattern(random)),
        3 => Formula::while_within(pattern(random), random.between(1, 6)),
        4 => Formula::hold(pattern(random), random.between(1, 5)),
        _ => {
            let open = if random.chance(1, 2) {
                OpenEnd::Head
            } else {
                OpenEnd::Extent
            };
            Formula::between_with(pattern(random), pattern(random), open)
        }
    }
}

fn formula(random: &mut Random, depth: u32) -> Formula {
    if depth == 0 || random.chance(1, 3) {
        return leaf(random);
    }
    match random.below(5) {
        0 => !formula(random, depth - 1),
        1 => formula(random, depth - 1).and(formula(random, depth - 1)),
        2 => formula(random, depth - 1).or(formula(random, depth - 1)),
        3 => formula(random, depth - 1).shift(random.between(-3, 3), random.between(-3, 3)),
        _ => formula(random, depth - 1).conjunctive_normal_form(),
    }
}

fn world(random: &mut Random) -> SequenceSystem {
    let mut system = SequenceSystem::new();
    for key in ["p", "q"] {
        let events = (0..random.below(8))
            .map(|_| {
                TimePointEvent::new(
                    random.between(DOMAIN_START, DOMAIN_END),
                    random.between(0, 4),
                )
            })
            .collect();
        system.insert(
            key,
            TimePointSequence::from_events(key, events).with_observed(extent(random)),
        );
    }
    for key in ["s", "t"] {
        system.insert(
            key,
            TimeIntervalSequence::from_spans(key, spans(random, 4)).with_observed(extent(random)),
        );
    }

    system
}

/// What `processor` derives from the members of `world`: the sequence it
/// produces, or the message of the failure.
fn derive(processor: &Processor, world: &SequenceSystem) -> Result<Sequence, String> {
    let mut system = StreamSystem::new();
    for (key, sequence) in world {
        system
            .insert_stream(key.clone(), sequence.clone())
            .map_err(|error| error.to_string())?;
    }
    system
        .add_processor(processor.clone())
        .map_err(|error| error.to_string())?;
    Ok(system
        .get(processor.output().as_str())
        .expect("the output is derived")
        .clone())
}

#[test]
fn random_processors_round_trip_and_evaluate_identically() {
    for_each_seed(CASES, |random, _| {
        let processor = Processor::new("o0", formula(random, 3));
        let loaded = round_trip(&processor);
        let world = world(random);

        assert_eq!(loaded, processor);
        assert!(loaded.check().is_ok());
        assert_eq!(derive(&loaded, &world), derive(&processor, &world));
    });
}

/// A processor for a pattern of every value type and for an output of every
/// kind.
fn every_kind(patterns: &[Pattern]) -> Vec<Processor> {
    let mut processors = Vec::new();
    for (index, pattern) in patterns.iter().enumerate() {
        processors.push(Processor::new(
            format!("occurs {index}"),
            Formula::occurs(pattern.clone()),
        ));
    }
    let by = Group::new("r", "id");
    processors.extend([
        Processor::new("edges", chronoloom::Events::Edges(Formula::state("up"))),
        Processor::new(
            "matching",
            chronoloom::Events::Matching(patterns[0].clone()),
        ),
        Processor::new("changes", chronoloom::Events::changes("i")),
        Processor::new(
            "projected",
            chronoloom::Events::project("r", "a", Kind::Float),
        ),
        Processor::new("each", Recipe::per_entity(by.clone(), Formula::state("up"))),
        Processor::new("any", Recipe::any_entity(by.clone(), Formula::state("up"))),
        Processor::new("number", Recipe::count_entities(by, Formula::state("up"))),
    ]);

    processors
}

fn every_pattern() -> [Pattern; 9] {
    [
        Pattern::new(
            "f",
            Condition::Within {
                low: 0.5,
                high: 2.5,
            },
        ),
        Pattern::new("i", Condition::In(vec![1_i64, 2])),
        Pattern::new("b", Condition::Equals(true)),
        Pattern::new("t", Condition::NotEquals(String::from("off"))),
        Pattern::new("v", Condition::Greater(Value::Float(5.0))),
        Pattern::new("v", Condition::Equals(Value::Integer(5))),
        Pattern::new("e", Condition::Equals(Edge::End)),
        Pattern::field("r", "a.b", Condition::Any),
        Pattern::field("r", "", Condition::Equals(Value::Null)),
    ]
}

#[test]
fn every_pattern_type_and_output_kind_round_trips() {
    let patterns = every_pattern();
    let processors = every_kind(&patterns);

    for processor in &processors {
        assert_eq!(&round_trip(processor), processor);
    }
    let key_type: Vec<&str> = patterns.iter().map(Pattern::value_type_name).collect();
    assert!(key_type.contains(&"chronoloom::values::value::Value"));
}

#[test]
fn a_processor_document_has_a_stable_shape() {
    let processor = Processor::new(
        "motion",
        Formula::between(
            Pattern::new("door", Condition::Equals(String::from("open"))),
            Pattern::field("door", "state", Condition::Less(Value::Integer(3))),
        ),
    );

    assert_eq!(
        serde_json::to_value(&processor).unwrap(),
        json!({
            "format": 1,
            "output": "motion",
            "formula": {"state": {"between": {
                "start": {"key": "door", "type": "text", "condition": {"equals": "open"}},
                "end": {
                    "key": "door", "path": "state", "type": "value",
                    "condition": {"less": 3}
                },
                "open": "extent"
            }}}
        })
    );
}

#[test]
fn a_pattern_on_another_value_type_cannot_be_saved() {
    let processor = Processor::new(
        "odd",
        Formula::occurs(Pattern::new("letters", Condition::Equals('a'))),
    );

    let error = serde_json::to_string(&processor).unwrap_err().to_string();
    assert!(
        error.contains("`letters`") && error.contains("char"),
        "{error}"
    );
}

fn processor_of(output: &str, formula: Json) -> Json {
    json!({"format": 1, "output": output, "formula": formula})
}

#[test]
fn a_processor_that_reads_its_own_output_is_refused() {
    let cycle = refusal::<Processor>(&processor_of("a", json!({"state": {"state": "a"}})));
    assert!(cycle.contains("circle"), "{cycle}");

    let chain: Processor = load(&processor_of("a", json!({"state": {"state": "b"}}))).unwrap();
    assert!(chain.check().is_ok());
}

#[test]
fn a_pattern_of_an_unknown_value_type_or_with_a_stray_path_is_refused() {
    let pattern = |extra: Json| {
        let mut base = json!({"key": "k", "type": "f64", "condition": "any"});
        for (name, value) in extra.as_object().unwrap() {
            base[name] = value.clone();
        }
        processor_of("a", json!({"state": {"occurs": base}}))
    };

    let unknown = refusal::<Processor>(&pattern(json!({"type": "decimal"})));
    assert!(
        unknown.contains("`k`") && unknown.contains("unsupported"),
        "{unknown}"
    );
    let stray = refusal::<Processor>(&pattern(json!({"path": "a"})));
    assert!(stray.contains("`k`") && stray.contains("path"), "{stray}");
    assert!(load::<Processor>(&pattern(json!({"type": "value", "path": "a"}))).is_ok());
}

#[test]
fn a_timeout_that_holds_nothing_is_refused() {
    let document = |timeout: i64| {
        processor_of(
            "a",
            json!({"state": {"while": {
                "pattern": {"key": "k", "type": "i64", "condition": "any"},
                "timeout": timeout
            }}}),
        )
    };

    for timeout in [0, -4] {
        assert!(refusal::<Processor>(&document(timeout)).contains("at least one tick"));
    }
    assert!(load::<Processor>(&document(1)).is_ok());
}

// ------------------------------------------------------- declared inputs

fn with_inputs(processor: Processor) -> Processor {
    processor
        .with_input("timeline", Input::Timeline)
        .with_input("floats", Input::Points(Kind::Float))
        .with_input("edges", Input::Edges)
        .with_input(
            "records",
            Input::Records(Schema::record([("id", Schema::Text)])),
        )
}

#[test]
fn declared_inputs_round_trip_and_are_left_out_when_there_are_none() {
    let processor = with_inputs(Processor::new("a", Formula::state("timeline")));
    let document = serde_json::to_value(&processor).unwrap();

    assert_eq!(
        document["inputs"],
        json!([
            {"key": "edges", "input": "edges"},
            {"key": "floats", "input": {"points": "float"}},
            {"key": "records", "input": {"records": {"record": {"id": "text"}}}},
            {"key": "timeline", "input": "timeline"},
        ])
    );
    assert_eq!(round_trip(&processor), processor);
    assert_eq!(round_trip(&processor).inputs(), processor.inputs());

    let plain = serde_json::to_value(Processor::new("a", Formula::state("b"))).unwrap();
    assert!(plain.get("inputs").is_none());
}

#[test]
fn an_input_declared_twice_is_refused() {
    let message = refusal::<Processor>(&json!({
        "format": 1,
        "inputs": [
            {"key": "a", "input": "timeline"},
            {"key": "a", "input": "edges"},
        ],
        "output": "o",
        "formula": {"state": {"state": "x"}},
    }));
    assert!(
        message.contains("`a`") && message.contains("twice"),
        "{message}"
    );
}

#[test]
fn a_document_without_declared_inputs_loads() {
    let loaded: Processor = load(&processor_of("a", json!({"state": {"state": "b"}}))).unwrap();
    assert!(loaded.declared("b").is_none());
}

// ----------------------------------------------------------- JSON Schema

#[cfg(feature = "json-schema")]
fn assert_matches_schema(processor: &Processor) {
    let schema = chronoloom::serialisation::processor_json_schema();
    let document = serde_json::to_value(processor).unwrap();
    let problems = validator::validate(&schema, &document);

    assert!(problems.is_empty(), "{problems:#?}\nin {document}");
}

#[cfg(feature = "json-schema")]
#[test]
fn the_schema_accepts_the_documents_of_random_processors() {
    for_each_seed(CASES, |random, _| {
        let mut processor = Processor::new("o0", formula(random, 3));
        if random.chance(1, 2) {
            processor = with_inputs(processor);
        }
        assert_matches_schema(&processor);
    });
}

#[cfg(feature = "json-schema")]
#[test]
fn the_schema_accepts_every_pattern_type_and_output_kind() {
    for processor in every_kind(&every_pattern()) {
        assert_matches_schema(&with_inputs(processor));
    }
    assert_matches_schema(&Processor::new(
        "timeout",
        Formula::while_within(Pattern::new("p", Condition::Any::<f64>), 3),
    ));
}

// ------------------------------------------------------------ a mixed use

#[test]
fn a_saved_system_and_processor_reproduce_a_derived_result() {
    let mut random = Random::new(5);
    let world = world(&mut random);
    let processor = Processor::new("a", Formula::state("s").or(Formula::state("t")));

    let derived = derive(&processor, &world).unwrap();
    let again = derive(&round_trip(&processor), &round_trip(&world)).unwrap();

    assert_eq!(again, derived);
    assert!(matches!(derived, Sequence::TimeInterval(_)));
}

// ------------------------------------------------------------- the README

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

    let documented = first_block(readme, "## Saving and loading", "```rust,ignore");
    let tested = first_block(
        module,
        "Saving and loading: the `serde` form of sequences, systems and processors,",
        "```",
    );

    assert!(!documented.is_empty());
    assert_eq!(documented, tested);
}
