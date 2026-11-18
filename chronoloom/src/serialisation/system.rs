//! A [`SequenceSystem`] as a document: its members in key order, each with the
//! state filed beside its sequence.

use std::collections::BTreeSet;

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::Timestamp;
use crate::processing::{Edge, Output};
use crate::sequence_system::{Key, MemberParts, Metadata, Sequence, SequenceSystem};
use crate::sequences::TimePointSequence;
use crate::values::{Schema, Value};

use super::sequences::{IntervalsBody, IntervalsBodyRef, PointsBody, PointsBodyRef};
use super::{member_error, Document, Format};

const SUPPORTED: &str = "f64, i64, bool, String, Value and Edge";

static EMPTY: Metadata = Metadata::new();

/// A sequence tagged with its value type, or `intervals` for a timeline.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename = "Sequence")]
enum SequenceRef<'a> {
    F64(PointsBodyRef<'a, f64>),
    I64(PointsBodyRef<'a, i64>),
    Bool(PointsBodyRef<'a, bool>),
    Text(PointsBodyRef<'a, String>),
    Value(PointsBodyRef<'a, Value>),
    Edge(PointsBodyRef<'a, Edge>),
    Intervals(IntervalsBodyRef<'a>),
}

impl<'a> SequenceRef<'a> {
    fn of(key: &Key, sequence: &'a Sequence) -> Result<Self, String> {
        let Sequence::TimePoint(points) = sequence else {
            return sequence
                .as_time_interval()
                .map(|timeline| Self::Intervals(IntervalsBodyRef::new(timeline)))
                .ok_or_else(|| member_error(key, "holds neither shape of sequence"));
        };

        sequence
            .as_time_point::<f64>()
            .map(|typed| Self::F64(PointsBodyRef::new(typed)))
            .or_else(|| {
                sequence
                    .as_time_point::<i64>()
                    .map(|typed| Self::I64(PointsBodyRef::new(typed)))
            })
            .or_else(|| {
                sequence
                    .as_time_point::<bool>()
                    .map(|typed| Self::Bool(PointsBodyRef::new(typed)))
            })
            .or_else(|| {
                sequence
                    .as_time_point::<String>()
                    .map(|typed| Self::Text(PointsBodyRef::new(typed)))
            })
            .or_else(|| {
                sequence
                    .as_time_point::<Value>()
                    .map(|typed| Self::Value(PointsBodyRef::new(typed)))
            })
            .or_else(|| {
                sequence
                    .as_time_point::<Edge>()
                    .map(|typed| Self::Edge(PointsBodyRef::new(typed)))
            })
            .ok_or_else(|| {
                member_error(
                    key,
                    format_args!(
                        "holds `{}` values, which the format does not support: it supports \
                         {SUPPORTED}",
                        points.value_type_name()
                    ),
                )
            })
    }
}

#[derive(Serialize)]
#[serde(rename = "Member")]
struct MemberRef<'a> {
    key: &'a Key,
    sequence: SequenceRef<'a>,
    metadata: &'a Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    frontier: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lineage: Option<&'a Output>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<&'a Schema>,
}

#[derive(Serialize)]
struct SystemBodyRef<'a> {
    metadata: &'a Metadata,
    members: Vec<MemberRef<'a>>,
}

impl Serialize for SequenceSystem {
    /// # Errors
    ///
    /// Refuses a point member whose value type is outside the closed set, and
    /// names its key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let members = self
            .iter()
            .map(|(key, sequence)| {
                Ok(MemberRef {
                    key,
                    sequence: SequenceRef::of(key, sequence).map_err(ser::Error::custom)?,
                    metadata: self.member_metadata(key.as_str()).unwrap_or(&EMPTY),
                    frontier: self.frontier_of(key.as_str()),
                    lineage: self.lineage(key.as_str()),
                    entity: self.lineage_entity(key.as_str()),
                    schema: self.schema(key.as_str()),
                })
            })
            .collect::<Result<_, S::Error>>()?;

        Document {
            format: Format,
            body: SystemBodyRef {
                metadata: self.metadata(),
                members,
            },
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SequenceBody {
    F64(PointsBody<f64>),
    I64(PointsBody<i64>),
    Bool(PointsBody<bool>),
    Text(PointsBody<String>),
    Value(PointsBody<Value>),
    Edge(PointsBody<Edge>),
    Intervals(IntervalsBody),
    #[serde(other)]
    Unsupported,
}

impl SequenceBody {
    /// The sequence, or `None` for a value type the format does not know.
    fn build(self, frontier: Option<Timestamp>) -> Result<Option<Sequence>, String> {
        Ok(Some(match self {
            Self::F64(body) => body.build(frontier)?.into(),
            Self::I64(body) => body.build(frontier)?.into(),
            Self::Bool(body) => body.build(frontier)?.into(),
            Self::Text(body) => body.build(frontier)?.into(),
            Self::Value(body) => body.build(frontier)?.into(),
            Self::Edge(body) => body.build(frontier)?.into(),
            Self::Intervals(body) => body.build(frontier)?.into(),
            Self::Unsupported => return Ok(None),
        }))
    }
}

#[derive(Deserialize)]
struct MemberBody {
    key: Key,
    sequence: SequenceBody,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    frontier: Option<Timestamp>,
    #[serde(default)]
    lineage: Option<Output>,
    #[serde(default)]
    entity: Option<Value>,
    #[serde(default)]
    schema: Option<Schema>,
}

impl MemberBody {
    /// The member, once everything filed beside its sequence agrees with it.
    fn build(self) -> Result<MemberParts, String> {
        let Self {
            key,
            sequence,
            metadata,
            frontier,
            lineage,
            entity,
            schema,
        } = self;

        let sequence = sequence
            .build(frontier)
            .map_err(|problem| member_error(&key, problem))?
            .ok_or_else(|| {
                member_error(
                    &key,
                    format_args!(
                        "holds values of a type the format does not support: it supports \
                         {SUPPORTED}"
                    ),
                )
            })?;

        if let Some(schema) = &schema {
            let points = sequence.as_time_point::<Value>().ok_or_else(|| {
                member_error(
                    &key,
                    "has a schema, which only a member of `value` points can have",
                )
            })?;
            check_schema(points, schema).map_err(|problem| member_error(&key, problem))?;
        }
        if lineage.is_some() && frontier.is_some() {
            return Err(member_error(
                &key,
                "is live and has a lineage, but a processor writes batch members",
            ));
        }
        check_entity(&key, lineage.as_ref(), entity.as_ref())?;

        Ok(MemberParts {
            key,
            sequence,
            metadata,
            frontier,
            lineage,
            entity,
            schema,
        })
    }
}

/// Every value of the member has the shape of its schema.
fn check_schema(points: &TimePointSequence<Value>, schema: &Schema) -> Result<(), String> {
    for event in points {
        if let Err(error) = schema.check(event.value()) {
            return Err(format!(
                "the value at {} does not have the schema, {error}",
                event.timestamp()
            ));
        }
    }

    Ok(())
}

/// An entity belongs to a member of a per-entity output, filed under the key
/// `<output>/<entity>`.
fn check_entity(key: &Key, lineage: Option<&Output>, entity: Option<&Value>) -> Result<(), String> {
    let Some(entity) = entity else {
        return Ok(());
    };
    if !matches!(lineage, Some(Output::PerEntity { .. })) {
        return Err(member_error(key, "has an entity but no per-entity lineage"));
    }
    if matches!(entity, Value::Null | Value::Record(_))
        || matches!(entity, Value::Float(number) if number.is_nan())
    {
        return Err(member_error(
            key,
            "has an entity that is not a bool, a number or text",
        ));
    }

    let name = crate::processing::entity_name(entity);
    if key
        .as_str()
        .strip_suffix(name.as_str())
        .and_then(|rest| rest.strip_suffix('/'))
        .is_none()
    {
        return Err(member_error(
            key,
            format_args!("is not filed under `<output>/{name}`, as the timeline of its entity"),
        ));
    }

    Ok(())
}

#[derive(Deserialize)]
struct SystemBody {
    #[serde(default)]
    metadata: Metadata,
    members: Vec<MemberBody>,
}

impl<'de> Deserialize<'de> for SequenceSystem {
    /// Builds every member through the checks of its own, so that each
    /// sequence keeps its invariants and what is filed beside it agrees.
    ///
    /// # Errors
    ///
    /// Refuses a member that breaks one, naming its key, and a key that appears
    /// twice.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<SystemBody>::deserialize(deserializer)?;

        let mut seen = BTreeSet::new();
        let mut members = Vec::with_capacity(body.members.len());
        for member in body.members {
            if !seen.insert(member.key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the key `{}` appears twice in the system",
                    member.key
                )));
            }
            members.push(member.build().map_err(de::Error::custom)?);
        }

        Ok(Self::from_parts(body.metadata, members))
    }
}
