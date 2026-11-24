//! A [`StreamSystem`] in batch mode as a document: its streams in key order, each
//! with the metadata and schema filed beside its recorded data, and its
//! processors in the order they are evaluated in. Derived streams are not
//! stored: loading evaluates the processors again.

use std::collections::BTreeSet;

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};

use crate::processing::Edge;
use crate::processor::Processor;
use crate::sequence_system::{Key, Metadata, Sequence};
use crate::stream_system::{Batch, StreamSystem};
use crate::values::{Schema, Value};

use super::processor::{ProcessorBody, ProcessorBodyRef};
use super::sequences::{IntervalsBody, IntervalsBodyRef, PointsBody, PointsBodyRef};
use super::{stream_error, Document, Format};

const SUPPORTED: &str = "f64, i64, bool, String, Value and Edge";

static EMPTY: Metadata = Metadata::new();

/// The `"mode"` field of a document: `"batch"`, the only mode a document holds.
struct BatchMode;

impl Serialize for BatchMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("batch")
    }
}

impl<'de> Deserialize<'de> for BatchMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let found = String::deserialize(deserializer)?;
        if found == "batch" {
            Ok(Self)
        } else {
            Err(de::Error::custom(format_args!(
                "unsupported mode `{found}`: a document holds a system in `batch` mode"
            )))
        }
    }
}

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
}

#[derive(Serialize)]
struct SystemBodyRef<'a> {
    mode: BatchMode,
    metadata: &'a Metadata,
    streams: Vec<StreamRef<'a>>,
    processors: Vec<ProcessorBodyRef<'a>>,
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
        let streams = self
            .streams()
            .map(|key| {
                let recorded = self.raw(key.as_str()).expect("a stream has recorded data");
                Ok(StreamRef {
                    key,
                    sequence: SequenceRef::of(key, recorded).map_err(ser::Error::custom)?,
                    metadata: self.member_metadata(key.as_str()).unwrap_or(&EMPTY),
                    schema: self.schema(key.as_str()),
                })
            })
            .collect::<Result<_, S::Error>>()?;

        Document {
            format: Format,
            body: SystemBodyRef {
                mode: BatchMode,
                metadata: self.metadata(),
                streams,
                processors: self.processors().iter().map(ProcessorBodyRef::of).collect(),
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
    fn build(self) -> Result<Option<Sequence>, String> {
        Ok(Some(match self {
            Self::F64(body) => body.build(None)?.into(),
            Self::I64(body) => body.build(None)?.into(),
            Self::Bool(body) => body.build(None)?.into(),
            Self::Text(body) => body.build(None)?.into(),
            Self::Value(body) => body.build(None)?.into(),
            Self::Edge(body) => body.build(None)?.into(),
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
}

#[derive(Deserialize)]
struct SystemBody {
    #[serde(rename = "mode")]
    _mode: BatchMode,
    #[serde(default)]
    metadata: Metadata,
    streams: Vec<StreamBody>,
    #[serde(default)]
    processors: Vec<ProcessorBody>,
}

impl<'de> Deserialize<'de> for StreamSystem<Batch> {
    /// Builds every stream through the checks of its own, then adds the
    /// processors in order, which evaluates the derived streams.
    ///
    /// # Errors
    ///
    /// Refuses a stream that breaks an invariant, naming its key, a key that
    /// appears twice, a schema its data does not fit, a processor that fails
    /// its own check, and a processor the system refuses or that fails over
    /// the streams, naming its output.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<SystemBody>::deserialize(deserializer)?;

        let mut system = Self::new();
        system.metadata_mut().clone_from(&body.metadata);

        let mut seen = BTreeSet::new();
        for stream in body.streams {
            let StreamBody {
                key,
                sequence,
                metadata,
                schema,
            } = stream;
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the key `{key}` appears twice in the system"
                )));
            }

            let sequence = sequence
                .build()
                .map_err(|problem| de::Error::custom(stream_error(&key, problem)))?
                .ok_or_else(|| {
                    de::Error::custom(stream_error(
                        &key,
                        format_args!(
                            "holds values of a type the format does not support: it supports \
                             {SUPPORTED}"
                        ),
                    ))
                })?;

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
