//! The failure recording into a system can return.

use core::fmt;

/// Why events could not be recorded into a [`SequenceSystem`].
///
/// Recording never creates a sequence: a sensor is declared with [`insert`]
/// before it reports, so a misspelt key is an error rather than a new,
/// silently separate sequence. Whatever the failure, nothing was recorded.
///
/// [`insert`]: super::SequenceSystem::insert
/// [`SequenceSystem`]: super::SequenceSystem
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordError {
    /// The system records no sequence for `key`.
    Unknown {
        /// The key nothing is recorded for.
        key: String,
    },

    /// Points were recorded into a timeline, or spans into a point sequence.
    ShapeMismatch {
        /// The sequence that holds the other shape.
        key: String,
    },

    /// Points were recorded into a point sequence whose events carry another
    /// value type.
    ValueTypeMismatch {
        /// The sequence the points were meant for.
        key: String,
        /// The value type the sequence holds.
        expected: &'static str,
        /// The value type the recorded points carried.
        found: &'static str,
    },
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { key } => write!(f, "no sequence for `{key}` in the system"),
            Self::ShapeMismatch { key } => write!(
                f,
                "the sequence for `{key}` holds the other shape of event from the one recorded"
            ),
            Self::ValueTypeMismatch {
                key,
                expected,
                found,
            } => write!(
                f,
                "the sequence for `{key}` holds `{expected}` values, not `{found}`"
            ),
        }
    }
}

impl std::error::Error for RecordError {}
