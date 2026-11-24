//! Checking a processor against the inputs it declares, without a system.

use core::any::type_name;

use crate::sequence_system::Key;
use crate::values::{Schema, Value};

use super::condition::{Condition, FieldCondition, Pattern};
use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;
use super::group;
use super::input::Input;
use super::kind::Kind;
use super::processor::Recipe;
use crate::processor::Processor;

/// How a read uses the fields of the records it reaches.
enum Usage<'a> {
    Field(&'a Condition<Value>),
    Project(Kind),
    Group,
}

/// What one leaf of an output reads.
enum Read<'a> {
    Timeline(&'a Key),
    Points {
        key: &'a Key,
        value_type: Option<&'static str>,
    },
    Records {
        key: &'a Key,
        path: &'a str,
        usage: Usage<'a>,
    },
}

impl Read<'_> {
    const fn key(&self) -> &Key {
        match self {
            Self::Timeline(key) | Self::Points { key, .. } | Self::Records { key, .. } => key,
        }
    }
}

fn pattern_read(pattern: &Pattern) -> Read<'_> {
    match pattern.condition.as_any().downcast_ref::<FieldCondition>() {
        Some(field) => Read::Records {
            key: &pattern.key,
            path: &field.path,
            usage: Usage::Field(&field.condition),
        },
        None => Read::Points {
            key: &pattern.key,
            value_type: Some(pattern.condition.value_type_name()),
        },
    }
}

/// The reads of a formula, in the order the tree is walked.
fn formula_reads<'a>(formula: &'a Formula, reads: &mut Vec<Read<'a>>) {
    match formula {
        Formula::State(key) => reads.push(Read::Timeline(key)),
        Formula::Between { start, end, .. } => {
            reads.push(pattern_read(start));
            reads.push(pattern_read(end));
        }
        Formula::While { pattern, .. } | Formula::Occurs(pattern) => {
            reads.push(pattern_read(pattern));
        }
        Formula::Not(inner) => formula_reads(inner, reads),
        Formula::And(formulas) | Formula::Or(formulas) => {
            for formula in formulas {
                formula_reads(formula, reads);
            }
        }
        Formula::Shift { formula, .. } => formula_reads(formula, reads),
    }
}

fn events_reads<'a>(events: &'a Events, reads: &mut Vec<Read<'a>>) {
    match events {
        Events::Edges(formula) => formula_reads(formula, reads),
        Events::Matching(pattern) => reads.push(pattern_read(pattern)),
        Events::Changes { key } => reads.push(Read::Points {
            key,
            value_type: None,
        }),
        Events::Project { key, path, into } => reads.push(Read::Records {
            key,
            path,
            usage: Usage::Project(*into),
        }),
    }
}

fn output_reads(output: &Recipe) -> Vec<Read<'_>> {
    let mut reads = Vec::new();
    match output {
        Recipe::Timeline(formula) => formula_reads(formula, &mut reads),
        Recipe::Points(events) => events_reads(events, &mut reads),
        Recipe::PerEntity { by, formula }
        | Recipe::AnyEntity { by, formula }
        | Recipe::CountEntities { by, formula } => {
            reads.push(Read::Records {
                key: &by.key,
                path: &by.path,
                usage: Usage::Group,
            });
            formula_reads(formula, &mut reads);
        }
    }
    reads
}

fn constants<'a>(condition: &'a Condition<Value>, found: &mut Vec<&'a Value>) {
    match condition {
        Condition::Any => {}
        Condition::Equals(value)
        | Condition::NotEquals(value)
        | Condition::Less(value)
        | Condition::LessOrEqual(value)
        | Condition::Greater(value)
        | Condition::GreaterOrEqual(value) => found.push(value),
        Condition::Within { low, high } => found.extend([low, high]),
        Condition::In(values) => found.extend(values),
        Condition::And(conditions) | Condition::Or(conditions) => {
            for condition in conditions {
                constants(condition, found);
            }
        }
        Condition::Not(inner) => constants(inner, found),
    }
}

/// Whether a field of the shape can ever equal or be ordered against `value`.
fn fits(schema: &Schema, value: &Value) -> bool {
    let mut shape = schema;
    while let Schema::Optional(inner) = shape {
        if matches!(value, Value::Null) {
            return true;
        }
        shape = inner;
    }
    let numeric = matches!(value, Value::Integer(_) | Value::Float(_));
    if numeric && matches!(shape, Schema::Integer | Schema::Float) {
        return true;
    }
    match (shape, value) {
        (Schema::Record(_), Value::Record(_)) => true,
        _ => shape.check(value).is_ok(),
    }
}

/// Whether a field of the shape can be read into `kind`, leniently: a field
/// that may be `Null` is not refused. The error is the kind of the field.
fn projects(schema: &Schema, kind: Kind) -> Result<(), &'static str> {
    let mut shape = schema;
    while let Schema::Optional(inner) = shape {
        shape = inner;
    }
    let found = match shape {
        Schema::Any | Schema::Optional(_) => return Ok(()),
        Schema::Null => "null",
        Schema::Bool => "bool",
        Schema::Integer => "integer",
        Schema::Float => "float",
        Schema::Text => "text",
        Schema::Record(_) => "record",
    };
    let accepted = match kind {
        Kind::Value => true,
        Kind::Bool => found == "bool",
        Kind::Integer => found == "integer",
        Kind::Float => matches!(found, "integer" | "float"),
        Kind::Text => found == "text",
    };
    if accepted {
        Ok(())
    } else {
        Err(found)
    }
}

/// The first thing wrong with `read` of the declared `input`.
fn check_read(read: &Read<'_>, input: &Input) -> Option<ProcessError> {
    let key = read.key().clone();
    match read {
        Read::Timeline(_) => {
            (*input != Input::Timeline).then_some(ProcessError::NotATimeline { key })
        }
        Read::Points { value_type, .. } => {
            let Some(expected) = input.value_type_name() else {
                return Some(ProcessError::NotPoints { key });
            };
            value_type.filter(|found| *found != expected).map(|found| {
                ProcessError::ValueTypeMismatch {
                    key,
                    expected,
                    found,
                }
            })
        }
        Read::Records { path, usage, .. } => {
            let Some(expected) = input.value_type_name() else {
                return Some(ProcessError::NotPoints { key });
            };
            if expected != type_name::<Value>() {
                return Some(ProcessError::ValueTypeMismatch {
                    key,
                    expected,
                    found: type_name::<Value>(),
                });
            }
            let schema = input.schema()?;
            let Some(field) = schema.at(path) else {
                return Some(ProcessError::MissingField {
                    key,
                    path: (*path).to_owned(),
                });
            };
            match usage {
                Usage::Group => None,
                Usage::Project(kind) => {
                    projects(field, *kind)
                        .err()
                        .map(|found| ProcessError::ProjectionMismatch {
                            key,
                            path: (*path).to_owned(),
                            expected: *kind,
                            found,
                        })
                }
                Usage::Field(condition) => {
                    let mut found = Vec::new();
                    constants(condition, &mut found);
                    found
                        .into_iter()
                        .find(|value| !fits(field, value))
                        .map(|value| ProcessError::FieldMismatch {
                            key,
                            path: (*path).to_owned(),
                            expected: field.describe(),
                            found: value.kind_name(),
                        })
                }
            }
        }
    }
}

/// Every problem `processor` has on its own, without a system, in a stable
/// order (see [`Processor`] for what is checked and in what order), or only the
/// first when `stop_at_first` is set. A transform reads its own output on
/// purpose: that is neither a cycle nor a dead declaration, and a read of it is
/// checked as any input.
///
/// # Errors
///
/// The problems found, never an empty list.
pub(crate) fn problems(
    processor: &Processor,
    stop_at_first: bool,
) -> Result<(), Vec<ProcessError>> {
    let transform = processor.is_transform();
    let output = processor.output();
    let recipe = processor.recipe();
    let per_entity = matches!(recipe, Recipe::PerEntity { .. });
    let mut problems = Vec::new();

    if !transform {
        let mut read = Vec::new();
        recipe.read_keys(&mut read);
        let cyclic = read.into_iter().any(|key| {
            if per_entity {
                group::is_under(key, output)
            } else {
                key == output
            }
        });
        if cyclic {
            problems.push(ProcessError::Cycle {
                keys: vec![output.clone()],
            });
            if stop_at_first {
                return Err(problems);
            }
        }
    }
    for key in processor.declared_inputs().keys() {
        if !transform && key == output {
            problems.push(ProcessError::InputIsOutput { key: key.clone() });
            if stop_at_first {
                return Err(problems);
            }
        }
    }

    for read in output_reads(recipe) {
        let key = read.key();
        let own = (!transform && output == key) || (per_entity && group::is_under(key, output));
        if own {
            continue;
        }
        let declared = processor.declared_inputs();
        let problem = match declared.get(key) {
            Some(input) => check_read(&read, input),
            None if declared.is_empty() => None,
            None => Some(ProcessError::UnknownKey { key: key.clone() }),
        };
        let found = problem.is_some();
        problems.extend(problem.map(|error| ProcessError::InOutput {
            output: output.clone(),
            error: Box::new(error),
        }));
        if stop_at_first && found {
            return Err(problems);
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}
