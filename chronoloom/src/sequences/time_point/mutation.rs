//! Changing a sequence, each method restoring the maintained time order
//! before it returns.

use crate::primitives::{TimePointEvent, Timestamp};

use super::TimePointSequence;

impl<T> TimePointSequence<T> {
    /// Drop every event, leaving the sequence empty.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Add an event, keeping the sequence ordered.
    ///
    /// Never displaces anything: an instant that already holds events gains
    /// another, placed after them. Amortized constant when the event belongs
    /// at the end — the usual case for events arriving in time order —
    /// otherwise a logarithmic search followed by a shift.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    ///
    /// let mut readings = TimePointSequence::new();
    /// readings.insert(TimePointEvent::new(10, 'a'));
    /// readings.insert(TimePointEvent::new(10, 'b'));
    ///
    /// let values: Vec<char> = readings.get(10).iter().map(|e| *e.value()).collect();
    /// assert_eq!(values, ['a', 'b']);
    /// ```
    pub fn insert(&mut self, event: TimePointEvent<T>) {
        match self.events.last() {
            // Out of order, so pay for the search and the shift.
            Some(last) if last.timestamp() > event.timestamp() => {
                let index = self.upper_bound(event.timestamp());
                self.events.insert(index, event);
            }
            // At or after the end: it already belongs where it lands, after
            // any events sharing its instant.
            _ => self.events.push(event),
        }
    }

    /// Remove every event at `timestamp` and return them in order.
    ///
    /// Returns an empty `Vec` when the instant held nothing. Costs a
    /// logarithmic search plus a shift of everything after the instant. Every
    /// event at that instant goes, so removing twice yields nothing the second
    /// time.
    pub fn remove(&mut self, timestamp: Timestamp) -> Vec<TimePointEvent<T>> {
        let start = self.lower_bound(timestamp);
        let end = self.upper_bound(timestamp);

        self.events.drain(start..end).collect()
    }
}
