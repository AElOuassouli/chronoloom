//! Set algebra over time points and intervals.
//!
//! `chronoloom` is a small, dependency-free library for reasoning about
//! temporal data: instants, half-open intervals, and the operations that
//! combine them.
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
//! let readings = TimePointSequence::from_events(vec![
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
//! let mut uptime = TimeIntervalSequence::new();
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

pub mod primitives;
pub mod sequences;

pub use primitives::{IntervalError, TimeIntervalEvent, TimePointEvent, Timestamp};
pub use sequences::{TimeIntervalSequence, TimePointSequence};
