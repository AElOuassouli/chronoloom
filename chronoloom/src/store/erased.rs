//! Point sequences with their value type erased, so sequences of different
//! types can sit side by side in one system.

use core::any::{self, Any};
use core::fmt;
use core::ops::Bound;

use crate::primitives::{TimePointEvent, Timestamp};
use crate::sequences::attribute::Attribute;
use crate::sequences::extent::Extent;
use crate::sequences::TimePointSequence;

/// What a point sequence's values must be to join a [`StreamSystem`].
///
/// Implemented for every type that qualifies, so it never needs implementing by
/// hand: `f64`, `String`, `bool`, and any value type of your own deriving
/// `Clone, Debug, PartialEq` all qualify. Each requirement buys the system one
/// property of its own — `Clone`, `Debug`, equality, and being shareable across
/// threads — and `'static` is what lets the value type be recovered after it
/// has been erased.
///
/// [`StreamSystem`]: crate::StreamSystem
pub trait PointValue: Clone + fmt::Debug + PartialEq + Send + Sync + 'static {}

impl<T: Clone + fmt::Debug + PartialEq + Send + Sync + 'static> PointValue for T {}

/// The operations a system needs from a point sequence without knowing its
/// value type. Private: [`AnyPointSequence`] is the public face.
pub(crate) trait ErasedPoints: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn clone_box(&self) -> Box<dyn ErasedPoints>;
    /// Equal when `other` holds the same value type and an equal sequence.
    fn equals(&self, other: &dyn ErasedPoints) -> bool;
    fn attribute(&self) -> &Attribute;
    fn set_attribute(&mut self, attribute: String);
    fn observed(&self) -> &Extent;
    fn observe(&mut self, extent: &Extent);
    fn observed_mut(&mut self) -> &mut Extent;
    fn slice_bounds(&self, range: (Bound<Timestamp>, Bound<Timestamp>)) -> Box<dyn ErasedPoints>;
    fn split_before(&mut self, frontier: Timestamp) -> Box<dyn ErasedPoints>;
    /// Append `next`, which comes after, and grow the extent by its own; false
    /// and untouched when `next` carries another value type.
    fn append(&mut self, next: &dyn ErasedPoints) -> bool;
    fn len(&self) -> usize;
    fn value_type_name(&self) -> &'static str;
    /// The sequence without the points equal to the sample before them.
    fn changes(&self) -> Box<dyn ErasedPoints>;
    /// The distinct instants holding an event in `[start, end)`, in order.
    fn instants_in(&self, start: Timestamp, end: Timestamp) -> Vec<Timestamp>;
}

impl<T: PointValue> ErasedPoints for TimePointSequence<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedPoints> {
        Box::new(self.clone())
    }

    fn equals(&self, other: &dyn ErasedPoints) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }

    fn attribute(&self) -> &Attribute {
        Self::attribute(self)
    }

    fn set_attribute(&mut self, attribute: String) {
        Self::set_attribute(self, attribute);
    }

    fn observed(&self) -> &Extent {
        Self::observed(self)
    }

    fn observe(&mut self, extent: &Extent) {
        Self::observe(self, extent);
    }

    fn observed_mut(&mut self) -> &mut Extent {
        Self::observed_mut(self)
    }

    fn instants_in(&self, start: Timestamp, end: Timestamp) -> Vec<Timestamp> {
        let mut instants: Vec<Timestamp> = self
            .range(start..end)
            .iter()
            .map(TimePointEvent::timestamp)
            .collect();
        instants.dedup();
        instants
    }

    fn slice_bounds(&self, range: (Bound<Timestamp>, Bound<Timestamp>)) -> Box<dyn ErasedPoints> {
        Box::new(self.slice(range))
    }

    fn split_before(&mut self, frontier: Timestamp) -> Box<dyn ErasedPoints> {
        Box::new(Self::split_before(self, frontier))
    }

    fn append(&mut self, next: &dyn ErasedPoints) -> bool {
        let Some(next) = next.as_any().downcast_ref::<Self>() else {
            return false;
        };

        // An event may lie outside its sequence's extent (as at the excluded
        // end of an observed span); inserting would grow the extent over it,
        // so the extent is the two extents put together.
        let mut observed = self.observed().clone();
        observed.grow(next.observed().as_slice());
        for event in next.iter() {
            self.insert(event.clone());
        }
        *self.observed_mut() = observed;
        true
    }

    fn len(&self) -> usize {
        Self::len(self)
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<T>()
    }

    fn changes(&self) -> Box<dyn ErasedPoints> {
        let events: Vec<&TimePointEvent<T>> = self.iter().collect();
        let mut kept = Vec::new();
        let mut cursor = 0;

        for run in self.observed().as_slice() {
            let (run_start, run_end) = run.bounds();
            while cursor < events.len() && events[cursor].timestamp() < run_start {
                cursor += 1;
            }

            let mut previous: Option<&T> = None;
            while cursor < events.len() && events[cursor].timestamp() < run_end {
                let event = events[cursor];
                cursor += 1;
                // Of the points sharing an instant, the last recorded is the sample.
                let is_sample = events
                    .get(cursor)
                    .is_none_or(|next| next.timestamp() != event.timestamp());
                if !is_sample {
                    continue;
                }
                if previous != Some(event.value()) {
                    kept.push(event.clone());
                }
                previous = Some(event.value());
            }
        }

        Box::new(TimePointSequence::from_parts(
            self.attribute().clone(),
            kept,
            self.observed().clone(),
        ))
    }
}

/// The operations an [`AnyValue`] needs from a value without knowing its type.
/// Private: [`AnyValue`] is the public face.
trait ErasedValue: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn clone_box(&self) -> Box<dyn ErasedValue>;
    /// Equal when `other` holds the same type and an equal value.
    fn equals(&self, other: &dyn ErasedValue) -> bool;
    fn value_type_name(&self) -> &'static str;
}

impl<T: PointValue> ErasedValue for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedValue> {
        Box::new(self.clone())
    }

    fn equals(&self, other: &dyn ErasedValue) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<T>()
    }
}

/// One point value whose type is known only at run time, as an
/// [`AnyPointSequence`] holds its values together: the way a processor hands
/// over the points of a member whose value type it does not know.
///
/// The type is checked when the value is read back with [`downcast_ref`] or
/// [`downcast`].
///
/// ```
/// use chronoloom::AnyValue;
///
/// let reading = AnyValue::new(21.5_f64);
///
/// assert_eq!(reading.downcast_ref::<f64>(), Some(&21.5));
/// assert!(reading.downcast_ref::<String>().is_none());
/// assert_eq!(reading, AnyValue::new(21.5_f64));
/// assert_ne!(reading, AnyValue::new(21_i64));
/// ```
///
/// [`downcast`]: AnyValue::downcast
/// [`downcast_ref`]: AnyValue::downcast_ref
pub struct AnyValue(Box<dyn ErasedValue>);

impl AnyValue {
    /// Erase `value`.
    #[must_use]
    pub fn new<T: PointValue>(value: T) -> Self {
        Self(Box::new(value))
    }

    /// The name of the value's type, as [`core::any::type_name`] spells it;
    /// meant for messages.
    #[must_use]
    pub fn value_type_name(&self) -> &'static str {
        self.0.value_type_name()
    }

    /// The value, if it is of type `T`.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }

    /// Recover the value, or get `self` back unchanged when it is not of type
    /// `T`.
    ///
    /// # Errors
    ///
    /// Returns `self` when `T` is not the value's type.
    pub fn downcast<T: 'static>(self) -> Result<T, Self> {
        if self.0.as_any().is::<T>() {
            let typed = self
                .0
                .into_any()
                .downcast()
                .expect("the type was checked just above");

            Ok(*typed)
        } else {
            Err(self)
        }
    }
}

impl Clone for AnyValue {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl fmt::Debug for AnyValue {
    /// The typed value's own `Debug`, as though it had never been erased.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl PartialEq for AnyValue {
    /// Equal when both are of the same type and the values are equal.
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(&*other.0)
    }
}

/// A [`TimePointSequence`] whose value type is known only at run time.
///
/// This is how a [`StreamSystem`] holds `f64` readings beside `String` log
/// lines: each sequence keeps its own value type, and the type is checked when
/// the sequence is read back with [`downcast_ref`] or [`downcast`]. Anything
/// that does not depend on the value type — the attribute, the number of events —
/// can be read without knowing it.
///
/// ```
/// use chronoloom::primitives::TimePointEvent;
/// use chronoloom::sequences::TimePointSequence;
/// use chronoloom::AnyPointSequence;
///
/// let temperature = TimePointSequence::from_events("temperature", vec![
///     TimePointEvent::new(10, 21.5_f64),
/// ]);
/// let erased = AnyPointSequence::from(temperature);
///
/// assert_eq!(erased.label(), "temperature");
/// assert!(erased.downcast_ref::<String>().is_none());
/// assert_eq!(erased.downcast_ref::<f64>().map(|s| s.len()), Some(1));
/// ```
///
/// There is deliberately no mutable downcast. Events reach a sequence inside a
/// stream system through an [`Update`] instead, and its attribute
/// changes through [`StreamSystem::relabel`].
///
/// [`downcast`]: AnyPointSequence::downcast
/// [`downcast_ref`]: AnyPointSequence::downcast_ref
/// [`StreamSystem`]: crate::StreamSystem
/// [`StreamSystem::relabel`]: crate::StreamSystem::relabel
/// [`Update`]: crate::Update
pub struct AnyPointSequence(pub(crate) Box<dyn ErasedPoints>);

impl AnyPointSequence {
    /// What the sequence is a record of.
    #[must_use]
    pub fn attribute(&self) -> &Attribute {
        self.0.attribute()
    }

    /// The sequence's attribute written out.
    #[must_use]
    pub fn label(&self) -> String {
        self.attribute().to_string()
    }

    /// The instants the sequence was observed over.
    #[must_use]
    pub fn observed(&self) -> &Extent {
        self.0.observed()
    }

    /// How many events the sequence holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the sequence holds no events at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The name of the value type the events carry, as
    /// [`core::any::type_name`] spells it.
    ///
    /// Meant for messages and debugging: the exact text is not guaranteed
    /// stable across compiler versions, so compare types with
    /// [`downcast_ref`] rather than with this.
    ///
    /// [`downcast_ref`]: AnyPointSequence::downcast_ref
    #[must_use]
    pub fn value_type_name(&self) -> &'static str {
        self.0.value_type_name()
    }

    /// The sequence, if its events carry values of type `T`.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&TimePointSequence<T>> {
        self.0.as_any().downcast_ref()
    }

    /// Recover the typed sequence, or get `self` back unchanged when its events
    /// do not carry values of type `T`.
    ///
    /// # Errors
    ///
    /// Returns `self` when `T` is not the sequence's value type.
    pub fn downcast<T: 'static>(self) -> Result<TimePointSequence<T>, Self> {
        if self.0.as_any().is::<TimePointSequence<T>>() {
            let typed = self
                .0
                .into_any()
                .downcast()
                .expect("the value type was checked just above");

            Ok(*typed)
        } else {
            Err(self)
        }
    }

    /// The sequence without the points equal to the sample before them, the
    /// first sample of each run of the extent kept.
    pub(crate) fn changes(&self) -> Self {
        Self(self.0.changes())
    }

    /// The distinct instants holding an event in `[start, end)`, in order.
    pub(crate) fn instants_in(&self, start: Timestamp, end: Timestamp) -> Vec<Timestamp> {
        self.0.instants_in(start, end)
    }

    /// Append `next`, the points that follow. The extent becomes the union of
    /// both extents; unlike `insert`, an event outside it does not grow it.
    /// False and unchanged when the two carry different value types.
    pub(crate) fn append(&mut self, next: &Self) -> bool {
        self.0.append(&*next.0)
    }

    /// Replace the attribute, for the system's own relabelling.
    pub(crate) fn set_attribute(&mut self, attribute: String) {
        self.0.set_attribute(attribute);
    }

    /// The type-erased sequence, for the system's own in-place recording.
    pub(crate) fn as_erased_mut(&mut self) -> &mut dyn ErasedPoints {
        &mut *self.0
    }
}

impl<T: PointValue> From<TimePointSequence<T>> for AnyPointSequence {
    fn from(sequence: TimePointSequence<T>) -> Self {
        Self(Box::new(sequence))
    }
}

impl Clone for AnyPointSequence {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl fmt::Debug for AnyPointSequence {
    /// The typed sequence's own `Debug`, as though it had never been erased.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl PartialEq for AnyPointSequence {
    /// Equal when both carry the same value type and the typed sequences are
    /// equal. Sequences of different value types are never equal, even when
    /// both are empty.
    ///
    /// Only `PartialEq`: the values need only be `PartialEq` themselves, and an
    /// `f64` reading of `NaN` is not equal even to itself.
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(&*other.0)
    }
}
