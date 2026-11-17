//! The failure evaluating a formula or running a processor can return.

use core::fmt;

use crate::sequence_system::Key;

/// Why a formula, an output or a processor could not be evaluated.
///
/// Evaluation reads a system and never changes it, and a processor applied to
/// a system either inserts every output or none, so whatever the failure,
/// nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProcessError {
    /// The system records no sequence for `key`.
    UnknownKey {
        /// The key nothing is recorded for.
        key: Key,
    },

    /// A state formula read `key`, which is a point sequence, not a timeline.
    NotATimeline {
        /// The point sequence that was read as a timeline.
        key: Key,
    },

    /// A pattern read `key`, which is a timeline, not a point sequence.
    NotPoints {
        /// The timeline that was read as points.
        key: Key,
    },

    /// A pattern's condition is over another value type than the points of
    /// the member it reads.
    ValueTypeMismatch {
        /// The point sequence the pattern reads.
        key: Key,
        /// The value type the member's points carry.
        expected: &'static str,
        /// The value type the pattern's condition is over.
        found: &'static str,
    },
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey { key } => write!(f, "no sequence for `{key}` in the system"),
            Self::NotATimeline { key } => {
                write!(f, "the sequence for `{key}` holds points, not a timeline")
            }
            Self::NotPoints { key } => {
                write!(f, "the sequence for `{key}` is a timeline, not points")
            }
            Self::ValueTypeMismatch {
                key,
                expected,
                found,
            } => write!(
                f,
                "the sequence for `{key}` holds `{expected}` values, \
                 but the condition is over `{found}`"
            ),
        }
    }
}

impl std::error::Error for ProcessError {}
