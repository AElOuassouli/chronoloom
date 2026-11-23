//! The data an update brings to a stream system.

use core::any::{self, Any};
use core::fmt;

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::sequences::TimePointSequence;
use crate::values::{Schema, SchemaError, Value};

use crate::sequence_system::{ErasedPoints, Key, PointValue};

/// Points and spans to record into the streams of a [`StreamSystem`], by key,
/// all or nothing.
///
/// An update maps keys to the events that arrived for them, such as one tick
/// of a sensor system. Each push gives its key and carries events of one value
/// type, so `f64` readings and `String` log lines can travel in the same
/// update.
///
/// ```
/// use chronoloom::{TimeIntervalEvent, TimePointEvent};
/// use chronoloom::stream_system::Update;
///
/// let mut update = Update::new();
/// update
///     .points("temperature", [TimePointEvent::new(10, 21.5_f64), TimePointEvent::new(11, 21.7)])
///     .points("log", [TimePointEvent::new(10, String::from("boot"))])
///     .spans("up", [TimeIntervalEvent::span(0, 10)?]);
///
/// assert_eq!(update.len(), 4);
/// # Ok::<(), chronoloom::IntervalError>(())
/// ```
///
/// Pushing is never checked against a system, because an update does not know
/// which system it is for. Every check happens when it is recorded, and the
/// update is recorded whole or not at all. Pushing to one key twice is fine:
/// both sets of events are recorded.
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Default)]
pub struct Update {
    /// In push order. Kept as pushed rather than grouped by key, so a second
    /// push to one key never has to reconcile its value type with the first —
    /// the system checks each against the sequence it is for.
    pub(crate) chunks: Vec<(Key, Pending)>,
}

impl Update {
    /// Create an empty update.
    #[must_use]
    pub const fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Add point events for the sequence for `key`.
    ///
    /// The events may be in any order; recording puts them in time order.
    pub fn points<T: PointValue>(
        &mut self,
        key: impl Into<Key>,
        events: impl IntoIterator<Item = TimePointEvent<T>>,
    ) -> &mut Self {
        let events: Vec<TimePointEvent<T>> = events.into_iter().collect();
        self.chunks
            .push((key.into(), Pending::Points(Box::new(events))));

        self
    }

    /// Add spans for the timeline for `key`.
    pub fn spans(
        &mut self,
        key: impl Into<Key>,
        spans: impl IntoIterator<Item = TimeIntervalEvent<()>>,
    ) -> &mut Self {
        self.chunks
            .push((key.into(), Pending::Spans(spans.into_iter().collect())));

        self
    }

    /// The update cut in two by key: the pushes for the keys `is_first` holds
    /// for, then the others, each in push order.
    pub(crate) fn split(self, is_first: impl Fn(&Key) -> bool) -> (Self, Self) {
        let (first, second) = self.chunks.into_iter().partition(|(key, _)| is_first(key));
        (Self { chunks: first }, Self { chunks: second })
    }

    /// How many events and spans the update carries, across every key.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.iter().map(|(_, pending)| pending.len()).sum()
    }

    /// Whether the update carries no events or spans at all.
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

    /// The earliest instant any event of the push starts at.
    pub(crate) fn earliest(&self) -> Option<Timestamp> {
        match self {
            Self::Points(events) => events.earliest(),
            Self::Spans(spans) => spans.iter().map(TimeIntervalEvent::start).min(),
        }
    }
}

/// A `Vec` of point events whose value type is known only at run time, and
/// can be checked against a sequence before it is moved into one.
pub(crate) trait ErasedEvents: fmt::Debug + Send + Sync {
    fn len(&self) -> usize;
    fn value_type_name(&self) -> &'static str;
    fn earliest(&self) -> Option<Timestamp>;
    /// The first event, in push order, that is a `Value` without the shape of
    /// `schema`; none when the events are of another value type.
    fn first_violation(&self, schema: &Schema) -> Option<SchemaError>;
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

    fn earliest(&self) -> Option<Timestamp> {
        self.iter().map(TimePointEvent::timestamp).min()
    }

    fn first_violation(&self, schema: &Schema) -> Option<SchemaError> {
        let as_values: &dyn Any = self;
        as_values
            .downcast_ref::<Vec<TimePointEvent<Value>>>()?
            .iter()
            .find_map(|event| schema.check(event.value()).err())
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
