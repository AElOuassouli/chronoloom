//! Set algebra over time points and intervals.
//!
//! `chronoloom` is a small library, with no dependencies unless the optional
//! `serde` feature is on, for reasoning about temporal data: instants,
//! half-open intervals, the operations that combine them, and processors that
//! derive timelines from events, on a finished recording or online as the data
//! arrives. This page names the types; the README walks through examples.
//!
//! # Primitives
//!
//! Two event shapes make up the vocabulary, both generic over the value they
//! carry — a measurement, a label, a set of tags, or nothing at all:
//!
//! - [`TimePointEvent`] anchors a value to a single instant, with no duration.
//! - [`TimeIntervalEvent`] attaches a value to a span of time.
//!
//! ```
//! use chronoloom::{TimeIntervalEvent, TimePointEvent};
//!
//! let reading = TimePointEvent::new(1_700_000_000, 21.5_f64);
//! let phase = TimeIntervalEvent::new(0, 60, "warm-up")?;
//!
//! assert_eq!(reading.timestamp(), 1_700_000_000);
//! assert_eq!(phase.duration(), 60);
//! # Ok::<(), chronoloom::IntervalError>(())
//! ```
//!
//! # Sequences
//!
//! [`TimePointSequence`] collects point events and keeps them in time order
//! however they arrive. Events sit contiguously, sorted by timestamp, so
//! lookups and windows binary-search that order rather than scanning, and a
//! window comes back as a real slice.
//!
//! ```
//! use chronoloom::{TimePointEvent, TimePointSequence};
//!
//! let readings = TimePointSequence::from_events("readings", vec![
//!     TimePointEvent::new(30, 3.0),
//!     TimePointEvent::new(10, 1.0),
//!     TimePointEvent::new(20, 2.0),
//! ]);
//!
//! let window: Vec<i64> = readings.range(10..30).iter().map(|e| e.timestamp()).collect();
//! assert_eq!(window, [10, 20]);
//!
//! assert_eq!(readings.nearest(28).map(|e| e.timestamp()), Some(30));
//! ```
//!
//! [`TimeIntervalSequence`] is the other shape: one state over time, as the
//! spans during which it was active. Overlapping and touching spans merge as
//! they arrive, so the timeline stays a canonical, disjoint description of which
//! instants are covered — two sequences are equal exactly when they cover the
//! same ones.
//!
//! ```
//! use chronoloom::{TimeIntervalEvent, TimeIntervalSequence};
//!
//! let mut uptime = TimeIntervalSequence::new("uptime");
//! uptime.insert(TimeIntervalEvent::span(0, 5)?);
//! uptime.insert(TimeIntervalEvent::span(20, 30)?);
//! uptime.insert(TimeIntervalEvent::span(5, 25)?);
//!
//! // The last span bridged the gap, so all three are one.
//! assert_eq!(uptime.len(), 1);
//! assert!(uptime.contains(12));
//! # Ok::<(), chronoloom::IntervalError>(())
//! ```
//!
//! # Sequence systems
//!
//! A [`SequenceSystem`] holds the sequences describing one subject side by
//! side, the way a sensor system holds its sensors: point sequences of any
//! value type next to timelines, one sequence per key. Events are
//! recorded into it as they arrive, one at a time or an [`Update`] at a time, all
//! or nothing.
//!
//! ```
//! use chronoloom::{SequenceSystem, TimeIntervalEvent, TimeIntervalSequence};
//! use chronoloom::{TimePointEvent, TimePointSequence};
//!
//! let mut machine = SequenceSystem::new();
//! machine.insert("temperature", TimePointSequence::<f64>::new("temperature"));
//! machine.insert("log", TimePointSequence::<String>::new("log"));
//! machine.insert("up", TimeIntervalSequence::new("up"));
//!
//! machine.record_point("temperature", TimePointEvent::new(10, 21.5_f64))?;
//! machine.record_point("log", TimePointEvent::new(10, String::from("boot")))?;
//! machine.record_span("up", TimeIntervalEvent::span(0, 60)?)?;
//!
//! let keys: Vec<&str> = machine.keys().map(|key| key.as_str()).collect();
//! assert_eq!(keys, ["log", "temperature", "up"]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What was observed
//!
//! Every sequence carries an [`Extent`], the instants it was observed over. An
//! instant inside it with no span is inactive; outside it, nothing is known.
//! Operations keep a result wherever it can be decided, and
//! [`TimeIntervalSequence::complement`] is the observed instants that are not
//! active.
//!
//! # Streaming
//!
//! A member inserted with [`SequenceSystem::insert_live`] is a stream with a
//! frontier: data before it is final, so recording an event before it is
//! [`RecordError::Late`]. [`SequenceSystem::advance`] moves the frontier and
//! marks what it passed as observed, and [`SequenceSystem::drain_finalised`]
//! hands the final part of every live member over, so a stream holds only its
//! open spans and its events not yet final.
//!
//! # Processors
//!
//! A [`Formula`] describes a state as data: active between one kind of event
//! and the next, while a value stays within bounds, held for a duration after
//! an event, or wherever other states are. A [`Processor`] is one recipe: the
//! inputs it reads, a formula over them, and the one output it produces; it is
//! how point sequences become timelines and timelines become [`Edge`] events.
//! A live [`StreamSystem`] runs them online and produces the same outputs as
//! in batch, chunk by chunk, as watermarks make the data final.
//! [`Processor::with_input`] declares what a processor expects of its inputs,
//! and [`Processor::validate`] lists every problem without a system.
//!
//! # Datasets
//!
//! A [`SequenceSystem`] is also a dataset. It files members under a [`Key`],
//! carries opaque [`Metadata`] on itself and on each member, remembers which
//! [`Recipe`] a derived member came from, and [`SequenceSystem::observe`] and
//! [`SequenceSystem::slice`] narrow every member to an extent or a window of
//! time at once. The README walks through an example.
//!
//! # Record values
//!
//! A [`Value`] is a dynamic payload (null, bool, integer, float, text or a record
//! of named fields) for a `TimePointSequence<Value>`, beside the sequences of
//! one fixed value type. A [`Schema`] says what shape the values of a member
//! must have ([`SequenceSystem::set_schema`]), and [`Events::Project`] reads a
//! field of each record out into a sequence of its own, online as well as in
//! batch. The README walks through an example.
//!
//! # One timeline per entity
//!
//! A [`Group`] names a member of `Value` records and the field that says which
//! entity each is about, and [`Recipe::per_entity`], [`Recipe::any_entity`] and
//! [`Recipe::count_entities`] build on it: one timeline per entity under
//! `<output key>/<entity>`, their union, and how many are active, as the changes
//! of a count. A live [`StreamSystem`] makes an entity when its first record is recorded.
//! The README walks through a presence example.
//!
//! # Stream systems
//!
//! A [`StreamSystem`] holds the streams of a subject and a list of
//! [`processor::Processor`]s, each one recipe: inputs, a formula, one
//! output. Its derived streams are read-only and follow every change: an
//! [`update`](StreamSystem::update) records into the streams and evaluates
//! again the processors that read what changed, and no others. It has a mode
//! as a type parameter: [`Batch`] holds a dataset whole, and [`Live`], made by
//! [`StreamSystem::live`], moves a frontier per stream with watermarks and runs
//! the processors over what became final, reporting a [`LiveReport`] of chunks.
//!
//! # Saving and loading
//!
//! With the `serde` feature, sequences, systems and processors are saved and loaded with any
//! self-describing format, as documents that start with a format version and
//! are validated when read, so a document that loads is always a valid value.
//! The `serialisation` module (present with the feature) documents the format,
//! and with the `json-schema` feature it also gives the JSON Schema of a
//! processor.
//!
//! # Intervals are half-open
//!
//! An interval spans `[start, end)` — `start` is included, `end` is excluded.
//! Two intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore share no
//! instant, and so do not intersect. A span is also never empty: `end` must be
//! strictly after `start`.
//!
//! ```
//! use chronoloom::TimeIntervalEvent;
//!
//! let a = TimeIntervalEvent::new(0, 5, "a")?;
//! let b = TimeIntervalEvent::new(5, 9, "b")?;
//!
//! assert_eq!(a.intersection(&b), None);
//! # Ok::<(), chronoloom::IntervalError>(())
//! ```
//!
//! Union takes the opposite view of the same fact: those two intervals together
//! cover exactly `[0, 9)` with no instant missing, so they merge.
//!
//! ```
//! use chronoloom::TimeIntervalEvent;
//!
//! let a = TimeIntervalEvent::new(0, 5, "a")?;
//! let b = TimeIntervalEvent::new(5, 9, "b")?;
//!
//! assert_eq!(a.union(&b), vec![TimeIntervalEvent::span(0, 9)?]);
//! # Ok::<(), chronoloom::IntervalError>(())
//! ```
//!
//! Python bindings are published separately as [`chronoloompy`] on PyPI.
//!
//! [`chronoloompy`]: https://pypi.org/project/chronoloompy/

#![warn(missing_docs)]

/// Compiles and runs the README's examples under `cargo test`, so its code
/// cannot drift away from the API it documents. Exists only during doctest
/// collection, and never appears in the rendered documentation.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;

mod operator;
pub mod primitives;
pub mod processing;
pub mod processor;
pub mod sequence_system;
pub mod sequences;
#[cfg(feature = "serde")]
pub mod serialisation;
pub mod stream_system;
pub mod values;

pub use primitives::{IntervalError, TimeIntervalEvent, TimePointEvent, Timestamp};
pub use processing::{
    Condition, Edge, Events, Formula, Group, Input, Kind, OpenEnd, Pattern, ProcessError, Recipe,
};
pub use processor::Processor;
pub use sequence_system::{
    AnyPointSequence, AnyValue, Key, Metadata, PointValue, RecordError, Sequence, SequenceSystem,
};
pub use sequences::{Attribute, Extent, SetOperation, TimeIntervalSequence, TimePointSequence};
pub use stream_system::{
    Batch, Chunk, Live, LiveReport, Mode, Report, StreamSystem, Update, UpdateError,
};
pub use values::{Schema, SchemaError, Value};
