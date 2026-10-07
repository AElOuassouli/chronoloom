//! The point event primitive: a value observed at a single instant.

use super::Timestamp;

/// A value observed at a single instant, with no duration.
///
/// The payload is generic and unconstrained, so an event can carry a
/// measurement, a label, a collection, or nothing at all (`()`).
///
/// Ordering is deliberately not derived: comparing events would otherwise fold
/// the payload into the comparison. Sort on [`timestamp`] instead.
///
/// ```
/// use chronoloom::primitives::TimePointEvent;
///
/// let mut events = vec![
///     TimePointEvent::new(30, 'c'),
///     TimePointEvent::new(10, 'a'),
///     TimePointEvent::new(20, 'b'),
/// ];
/// events.sort_by_key(TimePointEvent::timestamp);
///
/// let values: Vec<char> = events.iter().map(|e| *e.value()).collect();
/// assert_eq!(values, ['a', 'b', 'c']);
/// ```
///
/// [`timestamp`]: TimePointEvent::timestamp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimePointEvent<T> {
    timestamp: Timestamp,
    value: T,
}

impl<T> TimePointEvent<T> {
    /// Anchor `value` to `timestamp`.
    ///
    /// Any timestamp is valid, including zero and negative ones — the epoch is
    /// the caller's to choose.
    pub const fn new(timestamp: Timestamp, value: T) -> Self {
        Self { timestamp, value }
    }

    /// The instant this event is anchored to.
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// The value carried by this event.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consume the event and return its value, dropping the timestamp.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// Consume the event and return its timestamp and value.
    #[must_use]
    pub fn into_parts(self) -> (Timestamp, T) {
        (self.timestamp, self.value)
    }

    /// Transform the payload, keeping the same instant.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> TimePointEvent<U> {
        TimePointEvent {
            timestamp: self.timestamp,
            value: f(self.value),
        }
    }
}
