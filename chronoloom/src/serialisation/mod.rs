//! Saving and loading: the `serde` form of sequences, systems and processors,
//! behind the `serde` feature.
//!
//! Every type of the crate that holds data implements `Serialize` and
//! `Deserialize`, so any self-describing format works; JSON is the one the
//! tests and the examples use. Four types are documents on their own and
//! start with a format version, `{ "format": 1, .. }`: [`TimePointSequence`],
//! [`TimeIntervalSequence`], [`SequenceSystem`], [`Processor`] and
//! [`LiveProcessor`], the checkpoint of an online run. Everything
//! nested inside them (extents, events, attributes, formulas, values) carries
//! no version of its own, so the version belongs to the document and a nested
//! value is the same in every document.
//!
//! ```
//! use chronoloom::{
//!     Condition, Formula, Pattern, Processor, SequenceSystem, TimePointEvent, TimePointSequence,
//! };
//!
//! let mut system = SequenceSystem::new();
//! system.insert("count", TimePointSequence::from_events("count", vec![
//!     TimePointEvent::new(3, 4_i64),
//!     TimePointEvent::new(9, 8_i64),
//! ]));
//! system.metadata_mut().insert("unit".to_owned(), "seconds".to_owned());
//!
//! let saved = serde_json::to_string(&system).unwrap();
//! let loaded: SequenceSystem = serde_json::from_str(&saved).unwrap();
//! assert_eq!(loaded, system);
//!
//! // A processor is a document of its own, and a loaded one has been checked.
//! let processor = Processor::new(
//!     "busy",
//!     Formula::occurs(Pattern::new("count", Condition::Greater(5_i64))),
//! );
//! let rules = serde_json::to_string(&processor).unwrap();
//! let rules: Processor = serde_json::from_str(&rules).unwrap();
//! assert_eq!(rules, processor);
//!
//! // A reader refuses any format version it does not know.
//! let future = saved.replacen(r#""format":1"#, r#""format":2"#, 1);
//! assert!(serde_json::from_str::<SequenceSystem>(&future).is_err());
//! ```
//!
//! A checkpoint of a live processor is saved and restored the same way, and the
//! restored one goes on as one that never stopped:
//!
//! ```
//! use chronoloom::{
//!     Condition, Formula, LiveProcessor, Pattern, Processor, TimePointEvent,
//! };
//!
//! let busy = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
//! let mut live = LiveProcessor::new(&[Processor::new("busy", busy)], 0).unwrap();
//! live.on_point("count", TimePointEvent::new(7, 9_i64)).unwrap();
//! live.advance("count", 10).unwrap();
//!
//! // The host saves it with its own offsets, and a new process loads it.
//! let saved = serde_json::to_string(&live).unwrap();
//! let mut restored: LiveProcessor = serde_json::from_str(&saved).unwrap();
//! assert_eq!(restored.processors(), live.processors());
//! assert_eq!(restored.frontier("busy"), Some(10));
//! assert_eq!(restored.finish(30).unwrap(), live.finish(30).unwrap());
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
//!   places a span or an event outside it is refused, bar the two documented
//!   exceptions: a point stamped `i64::MAX` (see [`Extent`]), and the data of a
//!   live member of a system at or past its frontier, which is not final and
//!   lies outside the extent (see [`SequenceSystem`]). Before the frontier, a
//!   live member's data lies inside its extent like any other. The extent of a
//!   live member must stop at its frontier.
//! - A member of a system whose schema is stored must hold `Value` points that
//!   all have the shape of the schema, and a member's lineage and entity must
//!   agree with each other, with its key and with its frontier.
//! - A processor passes [`Processor::check`], and no key appears twice in a
//!   system or among the inputs a processor declares. A `While` timeout is positive.
//!
//! - A live processor passes the checks listed on [`LiveProcessor`]: its
//!   processors are valid, its inputs are the ones they read, and its entities,
//!   views, cursors and frontiers agree with each other.
//!
//! A refusal is the error of the format in use, and its message says what was
//! wrong and, for a member of a system, under which key.
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
//! - A system lists its members in key order, each with its key, its tagged
//!   sequence, its metadata, and, when it has them, its frontier, lineage,
//!   entity and schema. A processor is its `output` key, its `formula` and, when
//!   it declares any, its `inputs`. A live processor nests its processors and a
//!   system document, each with its own format number.
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
//! | format | change |
//! |---|---|
//! | 1 | the first version; a processor is one recipe (`output`, `formula`) and lists its declared `inputs`, left out when there are none |
//!
//! The live processor document was added under format 1 without changing it,
//! since no document of an earlier version of the crate can be affected. It
//! holds `processors` (a list of processor documents), `inputs` (a system document of
//! the live inputs and the views of the entities), `untyped` (the inputs that
//! have no value type yet, each with its key, frontier and events), `outputs`
//! (per output, `through`, `trusted` and the `cuts` of its `Between` leaves),
//! `groups` (per per-entity output, its entities in the order they were seen,
//! each with its `entity` value and `first` instant), `counts` and `finished`.
//!
//! [`Edge`]: crate::Edge
//! [`LiveProcessor`]: crate::LiveProcessor
//! [`Extent`]: crate::Extent
//! [`Pattern`]: crate::Pattern
//! [`Processor`]: crate::Processor
//! [`Processor::check`]: crate::Processor::check
//! [`SequenceSystem`]: crate::SequenceSystem
//! [`TimeIntervalSequence`]: crate::TimeIntervalSequence
//! [`TimeIntervalSequence::from_spans`]: crate::TimeIntervalSequence::from_spans
//! [`TimePointSequence`]: crate::TimePointSequence
//! [`Value`]: crate::Value

use core::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::Timestamp;

mod extent;
mod live;
mod processor;
#[cfg(feature = "json-schema")]
mod schema;
mod sequences;
mod system;
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

/// A message for an error that names a member of a system.
pub(crate) fn member_error(key: impl fmt::Display, problem: impl fmt::Display) -> String {
    format!("member `{key}`: {problem}")
}
