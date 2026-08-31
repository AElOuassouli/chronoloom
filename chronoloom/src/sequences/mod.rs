//! Ordered collections of temporal events.
//!
//! Where [`primitives`] describes a single event, a sequence holds many and
//! keeps them in time order however they arrive. The two shapes answer
//! different questions:
//!
//! - [`TimePointSequence`] is **many observations over time** — each event
//!   carries its own value, and several may share an instant.
//! - [`TimeIntervalSequence`] is **one state over time** — the spans during
//!   which that state was active. Every span means the same thing, so the
//!   sequence carries no per-span value, and overlapping spans merge into a
//!   canonical, disjoint timeline.
//!
//! ```
//! use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
//! use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};
//!
//! let mut readings = TimePointSequence::new();
//! readings.insert(TimePointEvent::new(30, 3.0_f64));
//! readings.insert(TimePointEvent::new(10, 1.0));
//!
//! let order: Vec<i64> = readings.iter().map(|e| e.timestamp()).collect();
//! assert_eq!(order, [10, 30]);
//!
//! let mut uptime = TimeIntervalSequence::new();
//! uptime.insert(TimeIntervalEvent::span(0, 10)?);
//! uptime.insert(TimeIntervalEvent::span(5, 20)?);
//!
//! // The two spans overlapped, so the timeline holds one.
//! assert_eq!(uptime.len(), 1);
//! # Ok::<(), chronoloom::primitives::IntervalError>(())
//! ```
//!
//! # Contiguous by design
//!
//! Events live in one contiguous `Vec`, kept sorted by timestamp. Lookups,
//! windows, and neighbour queries binary-search that maintained order, so they
//! are logarithmic; reading by position or as a slice is constant. Adding an
//! event that belongs at the end — the usual case for events arriving in time
//! order — is amortized constant, while inserting into the middle or removing
//! costs a shift of everything after the touched instant.
//!
//! Because whole events sit in memory, a sequence hands out ordinary
//! `&TimePointEvent<T>` references and real slices, which any code that has
//! never heard of `chronoloom` can consume. [`TimeIntervalSequence`] is built
//! the same way, with merging on top to keep its spans disjoint.
//!
//! ```
//! use chronoloom::primitives::TimePointEvent;
//! use chronoloom::sequences::TimePointSequence;
//!
//! let mut labels = TimePointSequence::new();
//! labels.insert(TimePointEvent::new(10, String::from("start")));
//! labels.insert(TimePointEvent::new(20, String::from("stop")));
//!
//! let event: &TimePointEvent<String> = labels.first().unwrap();
//! assert_eq!(event.value(), "start");
//!
//! let window: &[TimePointEvent<String>] = labels.range(15..);
//! assert_eq!(window.len(), 1);
//! ```
//!
//! [`primitives`]: crate::primitives

pub mod interval;
pub mod time_point;

pub use interval::TimeIntervalSequence;
pub use time_point::TimePointSequence;
