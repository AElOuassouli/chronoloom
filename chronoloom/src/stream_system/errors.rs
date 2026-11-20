//! Why a change to a stream system was refused.

use core::fmt;

use crate::processing::ProcessError;
use crate::sequence_system::{Key, RecordError};

/// Why a change to a [`StreamSystem`] was refused. Nothing changed in any
/// case.
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum UpdateError {
    /// Recording into, inserting or relabelling a stream failed.
    Record(RecordError),
    /// Evaluating a processor failed.
    Process {
        /// The output key of the processor.
        processor: Key,
        /// What went wrong.
        error: ProcessError,
    },
    /// The key belongs to a derived stream, which is read-only.
    Derived {
        /// The key of the derived stream.
        key: Key,
    },
    /// The key is already a stream, a derived stream or a processor's output.
    Taken {
        /// The key in use.
        key: Key,
    },
    /// The key is read by a processor, so it cannot be removed.
    Read {
        /// The key that is read.
        key: Key,
        /// The output key of a processor that reads it.
        by: Key,
    },
    /// No processor produces the key.
    UnknownProcessor {
        /// The key asked for.
        key: Key,
    },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "{error}"),
            Self::Process { processor, error } => {
                write!(f, "the processor of `{processor}` failed: {error}")
            }
            Self::Derived { key } => {
                write!(f, "`{key}` is a derived stream, which is read-only")
            }
            Self::Taken { key } => write!(f, "`{key}` is already in use in the system"),
            Self::Read { key, by } => {
                write!(f, "`{key}` is read by the processor of `{by}`")
            }
            Self::UnknownProcessor { key } => write!(f, "no processor produces `{key}`"),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Record(error) => Some(error),
            Self::Process { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl From<RecordError> for UpdateError {
    fn from(error: RecordError) -> Self {
        Self::Record(error)
    }
}
