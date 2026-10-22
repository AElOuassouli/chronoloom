//! Events for many sequences, gathered to be recorded in one call.

use core::any;
use core::fmt;

use crate::primitives::{TimeIntervalEvent, TimePointEvent};
use crate::sequences::TimePointSequence;

use super::erased::{ErasedPoints, PointValue};

/// Events for many sequences of a [`SequenceSystem`], recorded together by
/// [`record_batch`].
///
/// A batch maps attributes to the events that arrived for them — one tick of
/// a sensor system. Each push gives its attribute and carries events of one
/// value type, so `f64` readings and `String` log lines can travel in the same batch.
///
/// ```
/// use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
/// use chronoloom::sequence_system::Batch;
///
/// let mut batch = Batch::new();
/// batch
///     .points("temperature", [TimePointEvent::new(10, 21.5_f64), TimePointEvent::new(11, 21.7)])
///     .points("log", [TimePointEvent::new(10, String::from("boot"))])
///     .spans("up", [TimeIntervalEvent::span(0, 10)?]);
///
/// assert_eq!(batch.len(), 4);
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// Pushing is never checked against a system — a batch does not know which
/// system it is for. Every check happens when it is recorded, and the batch is
/// recorded whole or not at all. Pushing to one attribute twice is fine: both sets
/// of events are recorded.
///
/// [`record_batch`]: super::SequenceSystem::record_batch
/// [`SequenceSystem`]: super::SequenceSystem
#[derive(Debug, Default)]
pub struct Batch {
    /// In push order. Kept as pushed rather than grouped by attribute, so a second
    /// push to one attribute never has to reconcile its value type with the first —
    /// the system checks each against the sequence it is for.
    pub(crate) chunks: Vec<(String, Pending)>,
}

impl Batch {
    /// Create an empty batch.
    #[must_use]
    pub const fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Add point events for the sequence for `attribute`.
    ///
    /// The events may be in any order; recording puts them in time order.
    pub fn points<T: PointValue>(
        &mut self,
        attribute: impl Into<String>,
        events: impl IntoIterator<Item = TimePointEvent<T>>,
    ) -> &mut Self {
        let events: Vec<TimePointEvent<T>> = events.into_iter().collect();
        self.chunks
            .push((attribute.into(), Pending::Points(Box::new(events))));

        self
    }

    /// Add spans for the timeline for `attribute`.
    pub fn spans(
        &mut self,
        attribute: impl Into<String>,
        spans: impl IntoIterator<Item = TimeIntervalEvent<()>>,
    ) -> &mut Self {
        self.chunks.push((
            attribute.into(),
            Pending::Spans(spans.into_iter().collect()),
        ));

        self
    }

    /// How many events and spans the batch carries, across every attribute.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.iter().map(|(_, pending)| pending.len()).sum()
    }

    /// Whether the batch carries no events or spans at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The events one push carried, waiting to be recorded.
#[derive(Debug)]
pub(crate) enum Pending {
    Points(Box<dyn ErasedEvents>),
    Spans(Vec<TimeIntervalEvent<()>>),
}

impl Pending {
    fn len(&self) -> usize {
        match self {
            Self::Points(events) => events.len(),
            Self::Spans(spans) => spans.len(),
        }
    }
}

/// A `Vec` of point events whose value type is known only at run time, and
/// can be checked against a sequence before it is moved into one.
pub(crate) trait ErasedEvents: fmt::Debug + Send + Sync {
    fn len(&self) -> usize;
    fn value_type_name(&self) -> &'static str;
    /// Whether `target` is a point sequence these events can be recorded into.
    fn fits(&self, target: &dyn ErasedPoints) -> bool;
    /// Move the events into `target`.
    ///
    /// # Panics
    ///
    /// If `target` does not [fit](ErasedEvents::fits). Callers check first.
    fn record_into(self: Box<Self>, target: &mut dyn ErasedPoints);
}

impl<T: PointValue> ErasedEvents for Vec<TimePointEvent<T>> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<T>()
    }

    fn fits(&self, target: &dyn ErasedPoints) -> bool {
        target.as_any().is::<TimePointSequence<T>>()
    }

    fn record_into(self: Box<Self>, target: &mut dyn ErasedPoints) {
        target
            .as_any_mut()
            .downcast_mut::<TimePointSequence<T>>()
            .expect("the value type is checked before anything is recorded")
            .extend(*self);
    }
}
