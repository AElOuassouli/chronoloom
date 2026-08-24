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
//! # Intervals are half-open
//!
//! An interval spans `[start, end)` — `start` is included, `end` is excluded.
//! Two intervals that merely touch (`[0, 5)` and `[5, 9)`) therefore do not
//! overlap. A span is also never empty: `end` must be strictly after `start`.
//!
//! ```
//! use chronoloom::algebra::intersection;
//!
//! assert_eq!(intersection((0, 5), (3, 9)), Some((3, 5)));
//! ```
//!
//! Python bindings are published separately as [`chronoloompy`] on PyPI.
//!
//! [`chronoloompy`]: https://pypi.org/project/chronoloompy/

#![warn(missing_docs)]

pub mod algebra;
pub mod primitives;

pub use primitives::{IntervalError, TimeIntervalEvent, TimePointEvent, Timestamp};
