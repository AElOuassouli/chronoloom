//! What operators read from the members of a system.

use core::any;

use crate::primitives::Timestamp;
use crate::processing::{Pattern, ProcessError};
use crate::sequences::{Extent, TimePointSequence};
use crate::store::{AnyPointSequence, Key, Sequence, Store};
use crate::values::Value;

/// Whether each point of a member matches, in the member's order, and where it
/// was observed.
pub(super) struct Marks<'a> {
    /// Each point's timestamp, and whether its value matches.
    pub(super) points: Vec<(Timestamp, bool)>,
    /// The instants the member was observed over.
    pub(super) observed: &'a Extent,
}

/// The record points of the member `key`.
pub(crate) fn records<'a>(
    key: &Key,
    system: &'a Store,
) -> Result<&'a TimePointSequence<Value>, ProcessError> {
    records_of(key, system.get(key.as_str()))
}

/// The record points of `sequence`, the member `key` or a chunk of it.
pub(super) fn records_of<'a>(
    key: &Key,
    sequence: Option<&'a Sequence>,
) -> Result<&'a TimePointSequence<Value>, ProcessError> {
    match sequence {
        Some(Sequence::TimePoint(points)) => {
            points
                .downcast_ref::<Value>()
                .ok_or_else(|| ProcessError::ValueTypeMismatch {
                    key: key.clone(),
                    expected: points.value_type_name(),
                    found: any::type_name::<Value>(),
                })
        }
        Some(Sequence::TimeInterval(_)) => Err(ProcessError::NotPoints { key: key.clone() }),
        None => Err(ProcessError::UnknownKey { key: key.clone() }),
    }
}

/// The point sequence of the member `key`.
pub(super) fn points<'a>(
    key: &Key,
    system: &'a Store,
) -> Result<&'a AnyPointSequence, ProcessError> {
    match system.get(key.as_str()) {
        Some(Sequence::TimePoint(points)) => Ok(points),
        Some(Sequence::TimeInterval(_)) => Err(ProcessError::NotPoints { key: key.clone() }),
        None => Err(ProcessError::UnknownKey { key: key.clone() }),
    }
}

/// The error for a pattern whose condition is over another value type than
/// `points` carries.
pub(super) fn mismatch(pattern: &Pattern, points: &AnyPointSequence) -> ProcessError {
    ProcessError::ValueTypeMismatch {
        key: pattern.key.clone(),
        expected: points.value_type_name(),
        found: pattern.condition.value_type_name(),
    }
}

/// Which points of the member a pattern reads match, and where it was observed.
pub(super) fn read<'a>(pattern: &Pattern, system: &'a Store) -> Result<Marks<'a>, ProcessError> {
    let points = points(&pattern.key, system)?;
    let marks = pattern
        .condition
        .marks(points)
        .ok_or_else(|| mismatch(pattern, points))?;

    Ok(Marks {
        points: marks,
        observed: points.observed(),
    })
}
