//! Set algebra over time points and intervals.
//!
//! `chronoloom` is a small, dependency-free library for reasoning about
//! temporal data: instants, half-open intervals, and the operations that
//! combine them.
//!
//! Intervals are represented as `(start, end)` pairs of [`i64`] timestamps and
//! are **half-open** — `start` is included, `end` is excluded. Two intervals
//! that merely touch (`[0, 5)` and `[5, 9)`) therefore do not overlap.
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

pub mod algebra;
