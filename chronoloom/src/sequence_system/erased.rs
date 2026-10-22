//! Point sequences with their value type erased, so sequences of different
//! types can sit side by side in one system.

use core::any::{self, Any};
use core::fmt;

use crate::sequences::attribute::Attribute;
use crate::sequences::TimePointSequence;

/// What a point sequence's values must be to join a [`SequenceSystem`].
///
/// Implemented for every type that qualifies, so it never needs implementing by
/// hand: `f64`, `String`, `bool`, and any value type of your own deriving
/// `Clone, Debug, PartialEq` all qualify. Each requirement buys the system one
/// property of its own — `Clone`, `Debug`, equality, and being shareable across
/// threads — and `'static` is what lets the value type be recovered after it
/// has been erased.
///
/// [`SequenceSystem`]: super::SequenceSystem
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
    fn len(&self) -> usize;
    fn value_type_name(&self) -> &'static str;
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

    fn len(&self) -> usize {
        Self::len(self)
    }

    fn value_type_name(&self) -> &'static str {
        any::type_name::<T>()
    }
}

/// A [`TimePointSequence`] whose value type is known only at run time.
///
/// This is how a [`SequenceSystem`] holds `f64` readings beside `String` log
/// lines: each sequence keeps its own value type, and the type is checked when
/// the sequence is read back with [`downcast_ref`] or [`downcast`]. Anything
/// that does not depend on the value type — the attribute, the number of events —
/// can be read without knowing it.
///
/// ```
/// use chronoloom::primitives::TimePointEvent;
/// use chronoloom::sequences::TimePointSequence;
/// use chronoloom::sequence_system::AnyPointSequence;
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
/// There is deliberately no mutable downcast: a mutable sequence could be
/// given another attribute while a system files it under its old one. Events reach a
/// sequence inside a system through [`SequenceSystem::record_point`] instead.
///
/// [`downcast`]: AnyPointSequence::downcast
/// [`downcast_ref`]: AnyPointSequence::downcast_ref
/// [`SequenceSystem`]: super::SequenceSystem
/// [`SequenceSystem::record_point`]: super::SequenceSystem::record_point
pub struct AnyPointSequence(pub(crate) Box<dyn ErasedPoints>);

impl AnyPointSequence {
    /// What the sequence is a record of.
    #[must_use]
    pub fn attribute(&self) -> &Attribute {
        self.0.attribute()
    }

    /// The sequence's attribute written out — the text a system files it under.
    #[must_use]
    pub fn label(&self) -> String {
        self.attribute().to_string()
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
