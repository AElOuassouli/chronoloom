//! The point event primitive: a value observed at a single instant.

use super::Timestamp;

/// A value observed at a single instant, with no duration.
///
/// The payload is generic and unconstrained, so an event can carry a
/// measurement, a label, a collection, or nothing at all:
///
/// ```
/// use chronoloom::primitives::TimePointEvent;
///
/// let temperature = TimePointEvent::new(1_700_000_000, 21.5_f64);
/// let state = TimePointEvent::new(1_700_000_000, "booting".to_string());
/// let tick = TimePointEvent::new(1_700_000_000, ());
///
/// assert_eq!(temperature.timestamp(), state.timestamp());
/// assert_eq!(tick.timestamp(), 1_700_000_000);
/// ```
///
/// Ordering is deliberately not derived: comparing events would otherwise
/// fold the payload into the comparison. Sort on [`timestamp`] instead.
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
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// let before_epoch = TimePointEvent::new(-10, "prehistory");
    /// assert_eq!(before_epoch.timestamp(), -10);
    /// ```
    pub const fn new(timestamp: Timestamp, value: T) -> Self {
        Self { timestamp, value }
    }

    /// The instant this event is anchored to.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// assert_eq!(TimePointEvent::new(7, 'x').timestamp(), 7);
    /// ```
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// The value carried by this event.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// let event = TimePointEvent::new(7, "reading".to_string());
    /// assert_eq!(event.value(), "reading");
    /// ```
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consume the event and return its value, dropping the timestamp.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// let event = TimePointEvent::new(7, vec![1, 2, 3]);
    /// assert_eq!(event.into_value(), vec![1, 2, 3]);
    /// ```
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }

    /// Consume the event and return its timestamp and value.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// let (timestamp, value) = TimePointEvent::new(7, 'x').into_parts();
    /// assert_eq!((timestamp, value), (7, 'x'));
    /// ```
    #[must_use]
    pub fn into_parts(self) -> (Timestamp, T) {
        (self.timestamp, self.value)
    }

    /// Transform the payload, keeping the same instant.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    ///
    /// let raw = TimePointEvent::new(7, 21_i32);
    /// let labelled = raw.map(|v| format!("{v} degrees"));
    ///
    /// assert_eq!(labelled.timestamp(), 7);
    /// assert_eq!(labelled.value(), "21 degrees");
    /// ```
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> TimePointEvent<U> {
        TimePointEvent {
            timestamp: self.timestamp,
            value: f(self.value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TimePointEvent;
    use std::collections::BTreeSet;

    #[test]
    fn new_exposes_timestamp_and_value() {
        let event = TimePointEvent::new(42, 3.5_f64);

        assert_eq!(event.timestamp(), 42);
        assert_eq!(*event.value(), 3.5);
    }

    #[test]
    fn zero_and_negative_timestamps_are_accepted() {
        assert_eq!(TimePointEvent::new(0, ()).timestamp(), 0);
        assert_eq!(TimePointEvent::new(-1, ()).timestamp(), -1);
        assert_eq!(TimePointEvent::new(i64::MIN, ()).timestamp(), i64::MIN);
    }

    #[test]
    fn payload_may_be_a_non_copy_value() {
        let event = TimePointEvent::new(1, String::from("booting"));

        assert_eq!(event.value(), "booting");
        assert_eq!(event.into_value(), "booting");
    }

    #[test]
    fn payload_may_be_a_collection() {
        let tags = BTreeSet::from([String::from("alpha"), String::from("beta")]);
        let event = TimePointEvent::new(1, tags.clone());

        assert_eq!(event.value(), &tags);
    }

    #[test]
    fn into_parts_round_trips_the_constructor_arguments() {
        let event = TimePointEvent::new(9, 'z');

        assert_eq!(event.into_parts(), (9, 'z'));
    }

    #[test]
    fn map_transforms_the_value_and_preserves_the_timestamp() {
        let event = TimePointEvent::new(9, 2_i32);
        let mapped = event.map(|v| v.to_string());

        assert_eq!(mapped.timestamp(), 9);
        assert_eq!(mapped.value(), "2");
    }

    #[test]
    fn events_compare_on_both_fields() {
        assert_eq!(TimePointEvent::new(1, 'a'), TimePointEvent::new(1, 'a'));
        assert_ne!(TimePointEvent::new(1, 'a'), TimePointEvent::new(2, 'a'));
        assert_ne!(TimePointEvent::new(1, 'a'), TimePointEvent::new(1, 'b'));
    }
}
