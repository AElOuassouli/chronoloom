//! The determinism generator shared by the live processor tests: random
//! processors (entities, step values, field patterns and chains), and random
//! streams of events and watermarks that never record late.

use std::collections::BTreeMap;

use chronoloom::{
    Condition, Edge, Events, Formula, Group, Kind, OpenEnd, Pattern, Processor, Recipe,
    TimeIntervalEvent, Value,
};

use super::common::{extent, spans, Random, DOMAIN_END, DOMAIN_START};

const STEPS: u64 = 36;

/// The inputs a random processor may read.
pub const POINT_INPUTS: [&str; 2] = ["p", "q"];
pub const STATE_INPUTS: [&str; 2] = ["s", "t"];
pub const RECORD_INPUTS: [&str; 1] = ["r"];

/// The keys a formula may read, by what they hold.
struct Pools {
    states: Vec<String>,
    numbers: Vec<String>,
    edges: Vec<String>,
    records: Vec<String>,
}

/// A record the way a sensor message looks: some of the fields `n`, `flag`
/// and a nested `a` with its own `n`, each left out now and then.
fn record(random: &mut Random) -> Value {
    let mut fields: Vec<(&str, Value)> = Vec::new();
    if random.chance(2, 3) {
        fields.push(("n", Value::from(random.between(0, 4))));
    }
    if random.chance(1, 2) {
        fields.push(("flag", Value::from(random.chance(1, 2))));
    }
    if random.chance(1, 2) {
        let nested = if random.chance(2, 3) {
            vec![("n", Value::from(random.between(0, 4)))]
        } else {
            Vec::new()
        };
        fields.push(("a", Value::record(nested)));
    }

    Value::record(fields)
}

fn record_condition(random: &mut Random) -> Condition<Value> {
    match random.below(3) {
        0 => Condition::Equals(record(random)),
        1 => Condition::NotEquals(record(random)),
        _ => Condition::Any,
    }
}

/// A projection of a field of the records under `key`, and the kind it reads
/// into: every field is of the kind its name says, or absent.
fn projection(random: &mut Random, key: &str) -> (Events, Kind) {
    let (path, into) = match random.below(6) {
        0 => ("n", Kind::Integer),
        1 => ("a.n", Kind::Integer),
        2 => ("n", Kind::Float),
        3 => ("flag", Kind::Bool),
        4 => ("a", Kind::Value),
        _ => ("n", Kind::Value),
    };

    (Events::project(key, path, into), into)
}

fn number_condition(random: &mut Random) -> Condition<i64> {
    let bound = random.between(0, 4);
    match random.below(5) {
        0 => Condition::Equals(bound),
        1 => Condition::Less(bound),
        2 => Condition::GreaterOrEqual(bound),
        3 => Condition::Not(Box::new(Condition::Equals(bound))),
        _ => Condition::Any,
    }
}

fn edge_condition(random: &mut Random) -> Condition<Edge> {
    match random.below(3) {
        0 => Condition::Equals(Edge::Start),
        1 => Condition::Equals(Edge::End),
        _ => Condition::Any,
    }
}

fn pick(random: &mut Random, keys: &[String]) -> String {
    keys[random.below(keys.len() as u64) as usize].clone()
}

/// A condition on a field of a record, over the paths `record` fills and one
/// it never does.
fn field_pattern(random: &mut Random, pools: &Pools) -> Pattern {
    let path = ["n", "flag", "a.n", "a", "missing"][random.below(5) as usize];
    let bound = Value::from(random.between(0, 4));
    let condition = match random.below(7) {
        0 => Condition::Equals(bound),
        1 => Condition::Less(bound),
        2 => Condition::GreaterOrEqual(bound),
        3 => Condition::Equals(Value::from(random.chance(1, 2))),
        4 => Condition::Equals(Value::Null),
        5 => Condition::Not(Box::new(Condition::Equals(bound))),
        _ => Condition::Any,
    };

    Pattern::field(pick(random, &pools.records), path, condition)
}

fn pattern(random: &mut Random, pools: &Pools) -> Pattern {
    if random.chance(1, 4) {
        field_pattern(random, pools)
    } else if !pools.edges.is_empty() && random.chance(1, 3) {
        Pattern::new(pick(random, &pools.edges), edge_condition(random))
    } else {
        Pattern::new(pick(random, &pools.numbers), number_condition(random))
    }
}

fn leaf(random: &mut Random, pools: &Pools) -> Formula {
    match random.below(7) {
        0 => Formula::state(pick(random, &pools.states)),
        1 => Formula::occurs(pattern(random, pools)),
        2 => Formula::while_matching(pattern(random, pools)),
        3 => Formula::hold(pattern(random, pools), random.between(1, 5)),
        4 => Formula::while_within(pattern(random, pools), random.between(1, 9)),
        _ => {
            let open = if random.chance(1, 2) {
                OpenEnd::Head
            } else {
                OpenEnd::Extent
            };
            Formula::between_with(pattern(random, pools), pattern(random, pools), open)
        }
    }
}

fn formula(random: &mut Random, pools: &Pools, depth: u32) -> Formula {
    if depth == 0 || random.chance(1, 3) {
        return leaf(random, pools);
    }

    match random.below(4) {
        0 => !formula(random, pools, depth - 1),
        1 => {
            let (alpha, beta) = (random.between(-3, 4), random.between(-3, 4));
            formula(random, pools, depth - 1).shift(alpha, beta)
        }
        choice => {
            let parts = (0..random.between(2, 4))
                .map(|_| formula(random, pools, depth - 1))
                .collect();
            if choice == 2 {
                Formula::And(parts)
            } else {
                Formula::Or(parts)
            }
        }
    }
}

/// Up to four processors, each of which may read the outputs of the ones
/// before it.
pub fn processors(random: &mut Random) -> Vec<Processor> {
    let mut pools = Pools {
        states: STATE_INPUTS.map(String::from).to_vec(),
        numbers: POINT_INPUTS.map(String::from).to_vec(),
        edges: Vec::new(),
        records: RECORD_INPUTS.map(String::from).to_vec(),
    };
    let mut processors: Vec<Processor> = Vec::new();

    for index in 0..random.between(1, 5) {
        let key = format!("x{index}");
        match random.below(8) {
            6 | 7 => {
                // A timeline per entity of the records, and what is made of them.
                let path = ["n", "flag", "a.n"][random.below(3) as usize];
                let by = Group::new("r", path);
                let body = formula(random, &pools, 2);
                match random.below(3) {
                    0 => {
                        processors.push(Processor::new(key.as_str(), Recipe::per_entity(by, body)));
                    }
                    1 => {
                        processors.push(Processor::new(key.as_str(), Recipe::any_entity(by, body)));
                        pools.states.push(key);
                    }
                    _ => {
                        processors.push(Processor::new(
                            key.as_str(),
                            Recipe::count_entities(by, body),
                        ));
                        pools.numbers.push(key);
                    }
                }
            }
            0 => {
                processors.push(Processor::new(key.as_str(), formula(random, &pools, 3)));
                pools.states.push(key);
            }
            5 => {
                // The repeats of numbers or of records, whatever reads them.
                let numbers = random.chance(1, 2);
                let pool = if numbers {
                    &pools.numbers
                } else {
                    &pools.records
                };
                let from = pick(random, pool);
                processors.push(Processor::new(key.as_str(), Events::changes(from)));
                if numbers {
                    pools.numbers.push(key);
                } else {
                    pools.records.push(key);
                }
            }
            3 => {
                let read = Pattern::new(pick(random, &pools.records), record_condition(random));
                processors.push(Processor::new(key.as_str(), Events::Matching(read)));
                pools.records.push(key);
            }
            4 => {
                let from = pick(random, &pools.records);
                let (projected, into) = projection(random, &from);
                processors.push(Processor::new(key.as_str(), projected));
                match into {
                    Kind::Integer => pools.numbers.push(key),
                    Kind::Value => pools.records.push(key),
                    _ => {}
                }
            }
            1 => {
                processors.push(Processor::new(
                    key.as_str(),
                    Events::Edges(formula(random, &pools, 3)),
                ));
                pools.edges.push(key);
            }
            _ => {
                let read = Pattern::new(pick(random, &pools.numbers), number_condition(random));
                processors.push(Processor::new(key.as_str(), Events::Matching(read)));
                pools.numbers.push(key);
            }
        }
    }

    processors
}

// Random streams

#[derive(Debug, Clone)]
pub enum Op {
    Point(&'static str, i64, i64),
    Record(&'static str, i64, Value),
    Span(&'static str, i64, i64),
    Advance(&'static str, i64),
}

enum Pending {
    Points(Vec<(i64, i64)>),
    Records(Vec<(i64, Value)>),
    Spans(Vec<(i64, i64)>),
}

/// The inputs of one run, as a schedule of operations that never records late.
pub struct Stream {
    pub from: i64,
    pub ops: Vec<Op>,
    pub horizon: i64,
}

pub fn stream(random: &mut Random) -> Stream {
    let from = random.between(DOMAIN_START, 10);

    let mut pending: BTreeMap<&'static str, Pending> = BTreeMap::new();
    for key in POINT_INPUTS {
        let mut points: Vec<(i64, i64)> = (0..random.below(14))
            .map(|_| {
                (
                    random.between(DOMAIN_START, DOMAIN_END),
                    random.between(0, 4),
                )
            })
            .filter(|point| point.0 >= from)
            .collect();
        points.sort_by_key(|point| point.0);
        pending.insert(key, Pending::Points(points));
    }
    for key in RECORD_INPUTS {
        let mut records: Vec<(i64, Value)> = (0..random.below(14))
            .map(|_| (random.between(DOMAIN_START, DOMAIN_END), record(random)))
            .filter(|point| point.0 >= from)
            .collect();
        records.sort_by_key(|point| point.0);
        pending.insert(key, Pending::Records(records));
    }
    let drawn = [
        extent(random)
            .as_slice()
            .iter()
            .map(TimeIntervalEvent::bounds)
            .collect::<Vec<_>>(),
        spans(random, 5)
            .iter()
            .map(TimeIntervalEvent::bounds)
            .collect(),
    ];
    for (key, mut timeline) in STATE_INPUTS.into_iter().zip(drawn) {
        timeline.retain(|bounds| bounds.0 >= from);
        timeline.sort_by_key(|bounds| bounds.0);
        pending.insert(key, Pending::Spans(timeline));
    }

    let keys: Vec<&'static str> = pending.keys().copied().collect();
    let mut marks: BTreeMap<&'static str, i64> = keys.iter().map(|&key| (key, from)).collect();
    let mut ops = Vec::new();
    for _ in 0..STEPS {
        let key = keys[random.below(keys.len() as u64) as usize];
        let to = marks[key] + random.between(1, 9);
        let ahead = if random.chance(1, 3) { to + 4 } else { to };
        let batch = pending.get_mut(key).expect("every key has a stream");
        match batch {
            Pending::Points(points) => {
                while points.first().is_some_and(|point| point.0 < ahead) {
                    let (at, value) = points.remove(0);
                    ops.push(Op::Point(key, at, value));
                }
            }
            Pending::Records(records) => {
                while records.first().is_some_and(|point| point.0 < ahead) {
                    let (at, value) = records.remove(0);
                    ops.push(Op::Record(key, at, value));
                }
            }
            Pending::Spans(timeline) => {
                while timeline.first().is_some_and(|bounds| bounds.0 < ahead) {
                    let (start, end) = timeline.remove(0);
                    ops.push(Op::Span(key, start, end));
                }
            }
        }
        ops.push(Op::Advance(key, to));
        marks.insert(key, to);
    }

    let horizon = marks.values().copied().max().unwrap_or(from) + random.between(0, 6);
    Stream { from, ops, horizon }
}
