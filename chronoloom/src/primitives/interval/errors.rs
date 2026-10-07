//! The validation failure an interval constructor can return.

use core::fmt;

use crate::primitives::Timestamp;

/// Why a [`TimeIntervalEvent`] could not be built, or a bound could not be
/// moved.
///
/// [`TimeIntervalEvent`]: super::TimeIntervalEvent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IntervalError {
    /// `end` was not strictly after `start`, so the span would be empty or
    /// inverted.
    EndNotAfterStart {
        /// The rejected start bound.
        start: Timestamp,
        /// The rejected end bound.
        end: Timestamp,
    },

    /// Moving `bound` by `shift` landed outside the range a [`Timestamp`] can
    /// hold.
    ///
    /// Raised by [`TimeIntervalSequence::transform`], the one operation whose
    /// arithmetic is driven by caller-supplied numbers rather than by bounds
    /// that already exist.
    ///
    /// [`TimeIntervalSequence::transform`]: crate::sequences::TimeIntervalSequence::transform
    BoundOverflow {
        /// The bound that could not be moved.
        bound: Timestamp,
        /// How far it was asked to move.
        shift: Timestamp,
    },
}

impl fmt::Display for IntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndNotAfterStart { start, end } => write!(
                f,
                "interval end ({end}) must be strictly after its start ({start})"
            ),
            Self::BoundOverflow { bound, shift } => write!(
                f,
                "shifting bound ({bound}) by ({shift}) leaves the timestamp range"
            ),
        }
    }
}

impl std::error::Error for IntervalError {}
