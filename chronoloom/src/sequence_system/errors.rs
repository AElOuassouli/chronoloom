//! The failure recording into a system can return.

use core::fmt;

use crate::primitives::Timestamp;
use crate::values::SchemaError;

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

    /// An event arrived before the frontier of a live member, which is final
    /// up to there.
    Late {
        /// The live member the event was meant for.
        key: String,
        /// The earliest instant of the rejected events.
        timestamp: Timestamp,
        /// The frontier of the member: events must not start before it.
        frontier: Timestamp,
    },

    /// A [`Value`] did not have the shape of the schema of the member it was
    /// recorded into, or the schema asked for does not fit what the member
    /// already holds.
    ///
    /// [`Value`]: crate::Value
    SchemaMismatch {
        /// The member the value was meant for.
        key: String,
        /// Where and how the first offending value differs from the shape.
        error: SchemaError,
    },

    /// A live member was needed, and this one is a batch member.
    NotLive {
        /// The member that has no frontier.
        key: String,
    },

    /// The update carries watermarks, which a sequence system has no use for:
    /// they move the frontiers of a live stream system.
    Watermark,
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
            Self::Late {
                key,
                timestamp,
                frontier,
            } => write!(
                f,
                "an event at {timestamp} is late for `{key}`, whose data is final up to {frontier}"
            ),
            Self::SchemaMismatch { key, error } => {
                write!(f, "a value does not have the schema of `{key}`, {error}")
            }
            Self::NotLive { key } => write!(f, "the sequence for `{key}` is not a live member"),
            Self::Watermark => write!(f, "a sequence system has no frontier to move"),
        }
    }
}

impl std::error::Error for RecordError {}
