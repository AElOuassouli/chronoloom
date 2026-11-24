//! Saving and loading: the `serde` form of sequences, systems and processors,
//! behind the `serde` feature.
//!
//! Every type of the crate that holds data implements `Serialize` and
//! `Deserialize`, so any self-describing format works; JSON is the one the
//! tests and the examples use. Four types are documents on their own and
//! start with a format version, `{ "format": 1, .. }`: [`TimePointSequence`],
//! [`TimeIntervalSequence`], [`StreamSystem`] (in either mode) and [`Processor`]. Everything
//! nested inside them (extents, events, attributes, formulas, values) carries
//! no version of its own, so the version belongs to the document and a nested
//! value is the same in every document.
//!
//! ```
//! use chronoloom::{
//!     Condition, Formula, Pattern, Processor, StreamSystem, TimePointEvent, TimePointSequence,
//! };
//!
//! let mut system = StreamSystem::new();
//! system.insert_stream("count", TimePointSequence::from_events("count", vec![
//!     TimePointEvent::new(3, 4_i64),
//!     TimePointEvent::new(9, 8_i64),
//! ]))?;
//! system.add_processor(Processor::new(
//!     "busy",
//!     Formula::occurs(Pattern::new("count", Condition::Greater(5_i64))),
//! ))?;
//! system.metadata_mut().insert("unit".to_owned(), "seconds".to_owned());
//!
//! // The derived stream is not saved: loading evaluates the processor again.
//! let saved = serde_json::to_string(&system)?;
//! let loaded: StreamSystem = serde_json::from_str(&saved)?;
//! assert_eq!(loaded, system);
//!
//! // A processor is a document of its own, and a loaded one has been checked.
//! let processor = Processor::new(
//!     "busy",
//!     Formula::occurs(Pattern::new("count", Condition::Greater(5_i64))),
//! );
//! let rules = serde_json::to_string(&processor)?;
//! let rules: Processor = serde_json::from_str(&rules)?;
//! assert_eq!(rules, processor);
//!
//! // A reader refuses any format version it does not know.
//! let future = saved.replacen(r#""format":1"#, r#""format":2"#, 1);
//! assert!(serde_json::from_str::<StreamSystem>(&future).is_err());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A timeline, as the format writes it:
//!
//! ```
//! use chronoloom::{TimeIntervalEvent, TimeIntervalSequence};
//!
//! let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 10).unwrap()]);
//!
//! let text = serde_json::to_string(&up).unwrap();
//! assert_eq!(
//!     text,
//!     r#"{"format":1,"attribute":{"name":"up"},"transformation":[0,0],"observed":[[-9223372036854775808,9223372036854775807]],"spans":[[0,10]]}"#
//! );
//! assert_eq!(serde_json::from_str::<TimeIntervalSequence>(&text).unwrap(), up);
//! ```
//!
//! # Validation on read
//!
//! A document that loads always gives values whose invariants hold, however
//! it was written. Loading goes through the same constructors and checks as
//! building the value in code:
//!
//! - A span with `start >= end` is refused. Spans and extents that are
//!   unsorted, overlapping or touching are normalized, as
//!   [`TimeIntervalSequence::from_spans`] does.
//! - Point events are sorted by timestamp, events at one instant keeping the
//!   order they were stored in.
//! - Data lies inside the extent it was observed over, and a document that
//!   places a span or an event outside it is refused, bar one documented
//!   exception: a point stamped `i64::MAX` (see [`Extent`]).
//! - A stream of a system whose schema is stored must hold `Value` points that
//!   all have the shape of the schema.
//! - A processor passes [`Processor::check`], and no key appears twice in a
//!   system or among the inputs a processor declares. A `While` timeout is positive.
//! - A system is loaded as it is built in code: the streams are inserted, then
//!   the processors added in order. A processor the system refuses or that
//!   fails over the streams, a key a processor writes that a stream already
//!   holds, say, is the error of the load, naming its output.
//!
//! A refusal is the error of the format in use, and its message says what was
//! wrong and, for a stream of a system, under which key.
//!
//! # The closed set of value types
//!
//! Point sequences of a system and the patterns of a processor are generic over
//! a value type that the document cannot name by itself, so it is tagged with a
//! `"type"`, and a closed set of types is supported:
//!
//! | tag | Rust type |
//! |---|---|
//! | `"f64"` | `f64` |
//! | `"i64"` | `i64` |
//! | `"bool"` | `bool` |
//! | `"text"` | `String` |
//! | `"value"` | [`Value`] |
//! | `"edge"` | [`Edge`] |
//!
//! Saving a system with a point member of another type, or a processor with a
//! pattern on another type, is an error naming the key. A document with another
//! tag is an error naming the key too. A timeline member is tagged
//! `"intervals"`. A sequence that is a document on its own is typed by the
//! caller, `TimePointSequence<T>` for any serialisable `T`, and carries no tag.
//!
//! # Shapes
//!
//! - An [`Extent`] and the spans of a timeline are lists of `[start, end]`
//!   pairs. Names are lower case with underscores, and an enum is a string when
//!   it holds nothing and a one-key object when it does: `"any"`,
//!   `{"equals": 5}`.
//! - A [`Value`] is plain JSON: `null`, `true`, `5`, `5.0`, `"text"` and
//!   `{..}`. An integer and a float stay distinct on the way back, so `5` loads
//!   as `Value::Integer(5)` and `5.0` as `Value::Float(5.0)`, although the two
//!   compare equal. It needs a self-describing format, and a float that is not
//!   finite is refused when saving, since JSON has no form for it.
//! - A system in batch mode is `{ "format": 1, "mode": "batch", .. }` with its
//!   `metadata`, its `streams` in key order and its `processors` in the order
//!   they are evaluated in. A stream is its key, its tagged `sequence` as it was
//!   recorded (before any transform), its `metadata` and, when it has one, its
//!   `schema`. The derived streams are not stored, nor is their metadata: they
//!   are computed again on load, over the saved streams. A system whose derived
//!   streams are not what its processors give over its own streams, a
//!   [`slice`](crate::StreamSystem::slice) or the part a live system
//!   [drained](crate::StreamSystem::drain), loads with the derived streams the
//!   processors give over the saved ones, so it does not round trip. A
//!   processor in a system is the body of its own document, without the format
//!   number: its `output` key, its `formula` and, when it declares any, its
//!   `inputs`.
//! - A system in live mode is the same document with `"mode": "live"`, the
//!   instant it started at, `from`, and a `frontier` in each stream: a
//!   checkpoint. It also holds the running state of each processor, `nodes`,
//!   and the derived streams as the processors made them, `derived`; nothing
//!   is run on load, and the system goes on as the saved one would have, also
//!   after a [drain](crate::StreamSystem::drain). A stream without a frontier,
//!   a frontier before `from`, a schema (a live system holds none), a running
//!   state that contradicts its processor or the derived streams, and a
//!   document of the other mode are refused.
//!
//! # JSON Schema
//!
//! With the `json-schema` feature, [`processor_json_schema`] returns the JSON
//! Schema (draft 2020-12) of the processor document, so an editor or a server
//! can check a document without knowing the types of the crate. Most schemas
//! are derived with `schemars` from the same `serde` attributes as the JSON;
//! [`Value`], [`Pattern`] and the processor document, which are written by
//! hand, have hand-written schemas that describe what is actually written. A
//! system document has no schema yet. The descriptions in it come from the
//! documentation of the types.
//!
//! ```
//! # #[cfg(feature = "json-schema")]
//! # {
//! use chronoloom::serialisation::processor_json_schema;
//! use chronoloom::{Formula, Input, Kind, Processor};
//!
//! let processor = Processor::new("open", Formula::state("door").or(Formula::state("shut")))
//!     .with_input("door", Input::Timeline)
//!     .with_input("temperature", Input::Points(Kind::Float));
//!
//! // Once inputs are declared, `shut` must be one of them.
//! assert_eq!(processor.validate().unwrap_err().len(), 1);
//!
//! // The schema describes the document that is saved, `inputs` included.
//! let schema = processor_json_schema();
//! assert_eq!(schema["properties"]["format"]["const"], 1);
//! assert!(schema["properties"]["inputs"].is_object());
//! # }
//! ```
//!
//! The schema is kept in `tests/fixtures/processor.schema.json`, without its
//! descriptions, and a test fails on any difference. When a change to the
//! format is intended, decide first whether the format number must change (see
//! below), then run the tests with `CHRONOLOOM_UPDATE_SCHEMA=1` to rewrite the
//! fixture and review the difference.
//!
//! # Format versions
//!
//! The format number changes only when a document written by one version of the
//! crate can no longer be read by the next, and a reader refuses any number it
//! does not know. Fields that are added with a default do not change it.
//!
//! The running state of a live system's processors (`nodes`, and the chunks they
//! hold) is part of format 1, written from the private state of each operator. A
//! change to the state of an operator, a field or a tag renamed, is a change of
//! the format: decide whether the number must change. The test on
//! `tests/fixtures/live_checkpoint.json` fails on any difference, and is
//! rewritten like the processor schema, with `CHRONOLOOM_UPDATE_SCHEMA=1`.
//!
//! | format | change |
//! |---|---|
//! | 1 | the first version; a processor is one recipe (`output`, `formula`) and lists its declared `inputs`, left out when there are none; a system is its streams and processors, in batch mode or, with the running state of the processors and the derived streams, in live mode |
//!
//!
//! [`Edge`]: crate::Edge
//! [`Extent`]: crate::Extent
//! [`Pattern`]: crate::Pattern
//! [`Processor`]: crate::Processor
//! [`Processor::check`]: crate::Processor::check
//! [`StreamSystem`]: crate::StreamSystem
//! [`TimeIntervalSequence`]: crate::TimeIntervalSequence
//! [`TimeIntervalSequence::from_spans`]: crate::TimeIntervalSequence::from_spans
//! [`TimePointSequence`]: crate::TimePointSequence
//! [`Value`]: crate::Value

use core::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::Timestamp;

mod chunk;
mod extent;
mod processor;
#[cfg(feature = "json-schema")]
mod schema;
mod sequences;
mod stream_system;
mod value;

pub(crate) use extent::spans;
#[cfg(feature = "json-schema")]
pub use schema::processor_json_schema;

/// The format number of the documents this version of the crate writes and
/// reads. See the [module documentation](self) for what each number means.
pub const FORMAT_VERSION: u32 = 1;

/// The `"format"` field of a document: writes [`FORMAT_VERSION`], and refuses
/// any other number as soon as it is read.
pub(crate) struct Format;

impl Serialize for Format {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(FORMAT_VERSION)
    }
}

impl<'de> Deserialize<'de> for Format {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let found = u32::deserialize(deserializer)?;
        if found == FORMAT_VERSION {
            Ok(Self)
        } else {
            Err(de::Error::custom(format_args!(
                "unsupported format version {found}: this version of chronoloom reads version \
                 {FORMAT_VERSION}"
            )))
        }
    }
}

/// A body with the format number of a document in front of it.
#[derive(Serialize, Deserialize)]
pub(crate) struct Document<B> {
    pub(crate) format: Format,
    #[serde(flatten)]
    pub(crate) body: B,
}

/// Reads the timeout of a `While` formula, refusing one that holds nothing.
pub(crate) fn positive_timeout<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Timestamp>, D::Error> {
    match Option::<Timestamp>::deserialize(deserializer)? {
        Some(timeout) if timeout <= 0 => Err(de::Error::custom(format_args!(
            "a timeout holds at least one tick, got {timeout}"
        ))),
        timeout => Ok(timeout),
    }
}

/// A message for an error that names a stream of a system.
pub(crate) fn stream_error(key: impl fmt::Display, problem: impl fmt::Display) -> String {
    format!("stream `{key}`: {problem}")
}
