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

pub mod primitives;

pub use primitives::{IntervalError, TimeIntervalEvent, TimePointEvent, Timestamp};
