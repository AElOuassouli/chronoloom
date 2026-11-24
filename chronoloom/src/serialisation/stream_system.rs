//! A [`StreamSystem`] as a document, in batch mode or in live mode: its streams
//! in key order, each with the metadata and schema filed beside its recorded
//! data, and its processors in the order they are evaluated in. In batch mode
//! derived streams are not stored: loading evaluates the processors again. A
//! live system adds where it started, the frontier of each stream, the running
//! state of each processor and the derived streams as the processors made them:
//! nothing runs on load.

use std::collections::{BTreeMap, BTreeSet};

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::Timestamp;
use crate::processing::Edge;
use crate::processor::Processor;
use crate::store::{Key, Metadata, Sequence};
use crate::stream_system::checkpoint::{Checkpoint, CheckpointDerived, CheckpointStream};
use crate::stream_system::{Batch, Live, LiveNode, Mode, StreamSystem};
use crate::values::{Schema, Value};

use super::processor::{ProcessorBody, ProcessorBodyRef};
use super::sequences::{IntervalsBody, IntervalsBodyRef, PointsBody, PointsBodyRef};
use super::{stream_error, Document, Format};

pub(super) const SUPPORTED: &str = "f64, i64, bool, String, Value and Edge";

static EMPTY: Metadata = Metadata::new();

/// Refuse a document written for the other mode, and say how to cross.
fn expect_mode(found: &str, wanted: &str) -> Result<(), String> {
    if found == wanted {
        return Ok(());
    }
    let crossing = match wanted {
        "batch" => "a live system is made a batch one with `drain`",
        _ => "a batch system is made a live one with `into_live`",
    };
    Err(format!(
        "the document holds a system in `{found}` mode, not `{wanted}`: {crossing}"
    ))
}

/// A sequence tagged with its value type, or `intervals` for a timeline.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename = "Sequence")]
pub(super) enum SequenceRef<'a> {
    F64(PointsBodyRef<'a, f64>),
    I64(PointsBodyRef<'a, i64>),
    Bool(PointsBodyRef<'a, bool>),
    Text(PointsBodyRef<'a, String>),
    Value(PointsBodyRef<'a, Value>),
    Edge(PointsBodyRef<'a, Edge>),
    Intervals(IntervalsBodyRef<'a>),
}

impl<'a> SequenceRef<'a> {
    pub(super) fn of(key: &Key, sequence: &'a Sequence) -> Result<Self, String> {
        let Sequence::TimePoint(points) = sequence else {
            return sequence
                .as_time_interval()
                .map(|timeline| Self::Intervals(IntervalsBodyRef::new(timeline)))
                .ok_or_else(|| stream_error(key, "holds neither shape of sequence"));
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
                stream_error(
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
#[serde(rename = "Stream")]
struct StreamRef<'a> {
    key: &'a Key,
    sequence: SequenceRef<'a>,
    metadata: &'a Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<&'a Schema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frontier: Option<Timestamp>,
}

/// A derived stream of a checkpoint: its data and what names its entity.
#[derive(Serialize)]
#[serde(rename = "Derived")]
struct DerivedRef<'a> {
    key: &'a Key,
    sequence: SequenceRef<'a>,
    metadata: &'a Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<&'a Value>,
}

/// What a live system keeps besides its streams.
struct LiveRef<'a> {
    from: Timestamp,
    frontiers: BTreeMap<&'a Key, Timestamp>,
    nodes: &'a [LiveNode],
}

#[derive(Serialize)]
struct SystemBodyRef<'a> {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<Timestamp>,
    metadata: &'a Metadata,
    streams: Vec<StreamRef<'a>>,
    processors: Vec<ProcessorBodyRef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nodes: Option<&'a [LiveNode]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    derived: Vec<DerivedRef<'a>>,
}

/// Write the streams and processors of a system; `live` adds what a live
/// system keeps besides.
fn write<M: Mode, S: Serializer>(
    system: &StreamSystem<M>,
    mode: &'static str,
    live: Option<LiveRef<'_>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let streams = system
        .streams()
        .map(|key| {
            let recorded = system
                .raw(key.as_str())
                .expect("a stream has recorded data");
            Ok(StreamRef {
                key,
                sequence: SequenceRef::of(key, recorded).map_err(ser::Error::custom)?,
                metadata: system.member_metadata(key.as_str()).unwrap_or(&EMPTY),
                schema: system.schema(key.as_str()),
                frontier: live
                    .as_ref()
                    .and_then(|live| live.frontiers.get(key).copied()),
            })
        })
        .collect::<Result<_, S::Error>>()?;

    let derived = match &live {
        None => Vec::new(),
        Some(_) => system
            .derived()
            .map(|key| {
                let sequence = system.get(key.as_str()).expect("a derived stream is held");
                Ok(DerivedRef {
                    key,
                    sequence: SequenceRef::of(key, sequence).map_err(ser::Error::custom)?,
                    metadata: system.member_metadata(key.as_str()).unwrap_or(&EMPTY),
                    entity: system.entity(key.as_str()),
                })
            })
            .collect::<Result<_, S::Error>>()?,
    };

    Document {
        format: Format,
        body: SystemBodyRef {
            mode,
            from: live.as_ref().map(|live| live.from),
            nodes: live.as_ref().map(|live| live.nodes),
            derived,
            metadata: system.metadata(),
            streams,
            processors: system
                .processors()
                .iter()
                .map(ProcessorBodyRef::of)
                .collect(),
        },
    }
    .serialize(serializer)
}

impl Serialize for StreamSystem<Batch> {
    /// Saves the recorded data of the streams, so that a transformed stream is
    /// saved as it was recorded, and the processors, which give the rest back.
    /// A derived stream is not saved, nor is its metadata, and loading
    /// evaluates the processors again over the saved streams: a system whose
    /// derived streams differ from that, a slice or a drained part, loads with
    /// the others.
    ///
    /// # Errors
    ///
    /// Refuses a point stream whose value type is outside the closed set, and
    /// names its key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        write(self, "batch", None, serializer)
    }
}

impl Serialize for StreamSystem<Live> {
    /// Saves a checkpoint: the streams as held with the frontier of each, the
    /// processors, where the system started, the running state of each
    /// processor and the derived streams as the processors made them. Loading
    /// gives a system that goes on exactly as this one does.
    ///
    /// # Errors
    ///
    /// Refuses a point stream, a derived stream or a chunk the running state
    /// holds whose value type is outside the closed set.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let frontiers = self
            .streams()
            .filter_map(|key| Some((key, self.frontier(key.as_str())?)))
            .collect();
        let live = LiveRef {
            from: self.origin(),
            frontiers,
            nodes: self.nodes(),
        };
        write(self, "live", Some(live), serializer)
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum SequenceBody {
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
    pub(super) fn build(self) -> Result<Option<Sequence>, String> {
        self.assemble(false)
    }

    /// As [`build`](Self::build) for a sequence a live system holds, whose data
    /// may lie at the end of what it observed, as the `end` edge of a span does.
    pub(super) fn build_held(self) -> Result<Option<Sequence>, String> {
        self.assemble(true)
    }

    fn assemble(self, held: bool) -> Result<Option<Sequence>, String> {
        macro_rules! points {
            ($body:expr) => {
                if held {
                    $body.build_held()?.into()
                } else {
                    $body.build(None)?.into()
                }
            };
        }
        Ok(Some(match self {
            Self::F64(body) => points!(body),
            Self::I64(body) => points!(body),
            Self::Bool(body) => points!(body),
            Self::Text(body) => points!(body),
            Self::Value(body) => points!(body),
            Self::Edge(body) => points!(body),
            Self::Intervals(body) => body.build(None)?.into(),
            Self::Unsupported => return Ok(None),
        }))
    }
}

#[derive(Deserialize)]
struct StreamBody {
    key: Key,
    sequence: SequenceBody,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    schema: Option<Schema>,
    #[serde(default)]
    frontier: Option<Timestamp>,
}

#[derive(Deserialize)]
struct SystemBody {
    mode: String,
    #[serde(default)]
    from: Option<Timestamp>,
    #[serde(default)]
    metadata: Metadata,
    streams: Vec<StreamBody>,
    #[serde(default)]
    processors: Vec<ProcessorBody>,
    #[serde(default)]
    nodes: Option<Vec<LiveNode>>,
    #[serde(default)]
    derived: Vec<DerivedBody>,
}

#[derive(Deserialize)]
struct DerivedBody {
    key: Key,
    sequence: SequenceBody,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    entity: Option<Value>,
}

/// A stream of a document, built.
struct Loaded {
    key: Key,
    sequence: Sequence,
    metadata: Metadata,
    schema: Option<Schema>,
    frontier: Option<Timestamp>,
}

/// Build every stream of a document through the checks of its own.
fn build_streams<E: de::Error>(streams: Vec<StreamBody>) -> Result<Vec<Loaded>, E> {
    let mut seen = BTreeSet::new();
    let mut loaded = Vec::new();
    for stream in streams {
        let StreamBody {
            key,
            sequence,
            metadata,
            schema,
            frontier,
        } = stream;
        if !seen.insert(key.clone()) {
            return Err(E::custom(format_args!(
                "the key `{key}` appears twice in the system"
            )));
        }

        let sequence = sequence
            .build()
            .map_err(|problem| E::custom(stream_error(&key, problem)))?
            .ok_or_else(|| {
                E::custom(stream_error(
                    &key,
                    format_args!(
                        "holds values of a type the format does not support: it supports \
                         {SUPPORTED}"
                    ),
                ))
            })?;
        loaded.push(Loaded {
            key,
            sequence,
            metadata,
            schema,
            frontier,
        });
    }
    Ok(loaded)
}

impl<'de> Deserialize<'de> for StreamSystem<Batch> {
    /// Builds every stream through the checks of its own, then adds the
    /// processors in order, which evaluates the derived streams.
    ///
    /// # Errors
    ///
    /// Refuses a document of a live system, a stream that breaks an invariant,
    /// naming its key, a key that appears twice, a schema its data does not
    /// fit, a processor that fails its own check, and a processor the system
    /// refuses or that fails over the streams, naming its output.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<SystemBody>::deserialize(deserializer)?;
        expect_mode(&body.mode, "batch").map_err(de::Error::custom)?;

        let mut system = Self::new();
        system.metadata_mut().clone_from(&body.metadata);

        for stream in build_streams(body.streams)? {
            let Loaded {
                key,
                sequence,
                metadata,
                schema,
                ..
            } = stream;
            system
                .insert_stream(key.clone(), sequence)
                .map_err(|error| de::Error::custom(stream_error(&key, error)))?;
            if let Some(schema) = schema {
                system
                    .set_schema(key.as_str(), schema)
                    .map_err(|error| de::Error::custom(stream_error(&key, error)))?;
            }
            if let Some(kept) = system.member_metadata_mut(key.as_str()) {
                *kept = metadata;
            }
        }

        for processor in body.processors {
            let processor: Processor = processor.build().map_err(de::Error::custom)?;
            let output = processor.output().clone();
            system.add_processor(processor).map_err(|error| {
                de::Error::custom(format_args!("processor `{output}`: {error}"))
            })?;
        }

        Ok(system)
    }
}

impl<'de> Deserialize<'de> for StreamSystem<Live> {
    /// Builds every stream and derived stream through the checks of its own,
    /// and puts the processors back with their running state, without running
    /// anything.
    ///
    /// # Errors
    ///
    /// Refuses a document of a batch system, a stream that breaks an invariant
    /// or has a schema, naming its key, a key that appears twice, a stream
    /// without a frontier, a frontier before where the system started, a
    /// processor that fails its own check or that the system refuses, and a
    /// running state that contradicts its processor or the derived streams
    /// (another kind, entities that disagree, a stream observed past its
    /// frontier, an input read past its frontier).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<SystemBody>::deserialize(deserializer)?;
        expect_mode(&body.mode, "live").map_err(de::Error::custom)?;
        let from = body.from.ok_or_else(|| de::Error::missing_field("from"))?;
        let nodes = body
            .nodes
            .ok_or_else(|| de::Error::missing_field("nodes"))?;

        let mut streams = Vec::new();
        for stream in build_streams::<D::Error>(body.streams)? {
            let Loaded {
                key,
                sequence,
                metadata,
                schema,
                frontier,
            } = stream;
            if schema.is_some() {
                return Err(de::Error::custom(stream_error(
                    &key,
                    "a live system holds no schema",
                )));
            }
            let frontier = frontier.ok_or_else(|| {
                de::Error::custom(stream_error(
                    &key,
                    "has no frontier, which a live system needs",
                ))
            })?;
            streams.push(CheckpointStream {
                key,
                sequence,
                metadata,
                frontier,
            });
        }

        let mut derived = Vec::new();
        let mut seen = BTreeSet::new();
        for member in body.derived {
            if !seen.insert(member.key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the derived stream `{}` appears twice",
                    member.key
                )));
            }
            let sequence = member
                .sequence
                .build_held()
                .map_err(|problem| de::Error::custom(stream_error(&member.key, problem)))?
                .ok_or_else(|| {
                    de::Error::custom(stream_error(
                        &member.key,
                        format_args!(
                            "holds values of a type the format does not support: it supports \
                             {SUPPORTED}"
                        ),
                    ))
                })?;
            derived.push(CheckpointDerived {
                key: member.key,
                sequence,
                metadata: member.metadata,
                entity: member.entity,
            });
        }

        let mut processors = Vec::new();
        for processor in body.processors {
            processors.push(processor.build().map_err(de::Error::custom)?);
        }

        Self::restore(Checkpoint {
            from,
            metadata: body.metadata,
            streams,
            processors,
            nodes,
            derived,
        })
        .map_err(de::Error::custom)
    }
}
