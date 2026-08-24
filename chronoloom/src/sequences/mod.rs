//! Ordered collections of temporal events.
//!
//! Where [`primitives`] describes a single event, a sequence holds many and
//! keeps them in time order however they arrive. [`TimePointSequence`] is the
//! point-event shape; the interval shape follows.
//!
//! ```
//! use chronoloom::primitives::TimePointEvent;
//! use chronoloom::sequences::TimePointSequence;
//!
//! let mut readings = TimePointSequence::new();
//! readings.insert(TimePointEvent::new(30, 3.0_f64));
//! readings.insert(TimePointEvent::new(10, 1.0));
//! readings.insert(TimePointEvent::new(20, 2.0));
//!
//! let order: Vec<i64> = readings.iter().map(|e| e.timestamp()).collect();
//! assert_eq!(order, [10, 20, 30]);
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
//! never heard of `chronoloom` can consume.
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

pub mod time_point;

pub use time_point::TimePointSequence;
