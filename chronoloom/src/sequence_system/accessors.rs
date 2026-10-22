//! Reading a system: which attributes it records, and the sequence for each.

use std::collections::btree_map;

use crate::sequences::{TimeIntervalSequence, TimePointSequence};

use super::{Sequence, SequenceSystem};

impl SequenceSystem {
    /// How many sequences the system holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sequences.len()
    }

    /// Whether the system holds no sequences at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }

    /// Whether the system records a sequence for `attribute`.
    #[must_use]
    pub fn contains(&self, attribute: &str) -> bool {
        self.sequences.contains_key(attribute)
    }

    /// The sequence for `attribute`, of whichever shape.
    #[must_use]
    pub fn get(&self, attribute: &str) -> Option<&Sequence> {
        self.sequences.get(attribute)
    }

    /// The point sequence for `attribute`, if there is one and its events carry
    /// values of type `T`.
    ///
    /// `None` does not say which of those failed; [`get`] does.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    /// use chronoloom::sequence_system::SequenceSystem;
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert(TimePointSequence::from_events("temperature", vec![
    ///     TimePointEvent::new(10, 21.5_f64),
    /// ]));
    ///
    /// assert!(system.points::<f64>("temperature").is_some());
    /// assert!(system.points::<String>("temperature").is_none());
    /// assert!(system.points::<f64>("humidity").is_none());
    /// ```
    ///
    /// [`get`]: SequenceSystem::get
    #[must_use]
    pub fn points<T: 'static>(&self, attribute: &str) -> Option<&TimePointSequence<T>> {
        self.get(attribute)?.as_time_point()
    }

    /// The timeline for `attribute`, if there is one.
    #[must_use]
    pub fn intervals(&self, attribute: &str) -> Option<&TimeIntervalSequence> {
        self.get(attribute)?.as_time_interval()
    }

    /// Every attribute the system records, in order.
    pub fn attributes(&self) -> btree_map::Keys<'_, String, Sequence> {
        self.sequences.keys()
    }

    /// Walk every sequence with its attribute, in attribute order.
    pub fn iter(&self) -> btree_map::Iter<'_, String, Sequence> {
        self.sequences.iter()
    }
}
