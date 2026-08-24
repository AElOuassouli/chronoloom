//! The event primitives: values anchored to an instant or to a span of time.
//!
//! Two shapes cover the vocabulary:
//!
//! - [`TimePointEvent`] — a value observed at a single instant, with no
//!   duration.
//! - [`TimeIntervalEvent`] — a value attached to a half-open span
//!   `[start, end)`.
//!
//! Both are generic over their payload, so a value can be anything: a
//! measurement, a label, a set of tags, or nothing at all (`()`).
//!
//! ```
//! use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
//!
//! let reading = TimePointEvent::new(1_700_000_000, 21.5_f64);
//! let window = TimeIntervalEvent::new(0, 60, "warm-up")?;
//!
//! assert_eq!(reading.timestamp(), 1_700_000_000);
//! assert_eq!(window.duration(), 60);
//! # Ok::<(), chronoloom::primitives::IntervalError>(())
//! ```

pub mod interval;
pub mod time_point;

pub use interval::{IntervalError, TimeIntervalEvent};
pub use time_point::TimePointEvent;

/// A point on the timeline, as an integer count of ticks.
///
/// The unit is deliberately unspecified — seconds, milliseconds, nanoseconds,
/// or any application-defined epoch. `chronoloom` never interprets a timestamp;
/// it only compares and subtracts them. Negative values are ordinary, so an
/// epoch may sit anywhere.
///
/// ```
/// use chronoloom::primitives::Timestamp;
///
/// let t: Timestamp = -42;
/// assert!(t < 0);
/// ```
pub type Timestamp = i64;
