//! The failure evaluating a formula or running a processor can return.

use core::fmt;

use crate::primitives::Timestamp;
use crate::sequence_system::Key;
use crate::values::SchemaError;

use super::kind::Kind;

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

    /// The outputs of a processor read each other in a circle.
    Cycle {
        /// The keys of the outputs on the cycle, in key order. A cycle of one
        /// is an output that reads itself.
        keys: Vec<Key>,
    },

    /// A projection met a value that is not of the kind it projects into.
    ProjectionMismatch {
        /// The member the projection reads.
        key: Key,
        /// The path of the field projected.
        path: String,
        /// The kind the projection asked for.
        expected: Kind,
        /// The kind name of the value found at the path, as
        /// [`Value::kind_name`](crate::Value::kind_name) spells it.
        found: &'static str,
    },

    /// A derived member has a schema, and what its lineage evaluates to again
    /// does not have its shape.
    SchemaMismatch {
        /// The member that was derived again.
        key: Key,
        /// Where and how the first offending value differs from the shape.
        error: SchemaError,
    },

    /// Two members of a processor's result would be filed under one key: two
    /// entities of a [`PerEntity`] output whose names read the same (the text
    /// `5` and the integer `5`), or an entity and another output.
    ///
    /// [`PerEntity`]: super::Recipe::PerEntity
    DuplicateMember {
        /// The key two members would share.
        key: Key,
    },

    /// An output of the kind [`PerEntity`] has one member per entity, so it has
    /// no single sequence to evaluate.
    ///
    /// [`PerEntity`]: super::Recipe::PerEntity
    SeveralMembers,

    /// A [`LiveProcessor`] cannot run an output that reads `key`: a member of a
    /// [`PerEntity`] output, whose entities it only learns as events arrive, or
    /// an output that a group is made of.
    ///
    /// [`LiveProcessor`]: super::LiveProcessor
    /// [`PerEntity`]: super::Recipe::PerEntity
    GroupedRead {
        /// The key that is read.
        key: Key,
    },

    /// A declared [`Input`] has a schema with no field at `path`, which a
    /// field pattern, a projection or a group of the processor reads.
    ///
    /// [`Input`]: super::Input
    MissingField {
        /// The declared input that was read.
        key: Key,
        /// The dotted path the schema lacks.
        path: String,
    },

    /// A field pattern compares the field at `path` of the declared input
    /// `key` with a constant its schema never gives.
    FieldMismatch {
        /// The declared input that was read.
        key: Key,
        /// The dotted path of the field.
        path: String,
        /// The shape the schema gives the field.
        expected: String,
        /// The kind of the constant.
        found: &'static str,
    },

    /// `key` is declared as an input and is also an output of the processor,
    /// so every read of it reads the output and the declaration is dead.
    InputIsOutput {
        /// The key that is both.
        key: Key,
    },

    /// A processor that transforms the stream `key` produced a sequence of
    /// another shape: a transform keeps points as points and a timeline as a
    /// timeline.
    TransformShape {
        /// The stream the processor transforms.
        key: Key,
        /// The shape the stream has: `points` or `timeline`.
        expected: &'static str,
        /// The shape the processor produced.
        found: &'static str,
    },

    /// `error` was found in the output filed under `output`; what
    /// [`Processor::validate`] returns, so that every error names its output.
    ///
    /// [`Processor::validate`]: crate::processor::Processor::validate
    InOutput {
        /// The key of the output.
        output: Key,
        /// What is wrong with it.
        error: Box<ProcessError>,
    },

    /// An event reached a [`LiveProcessor`] before the frontier of its input,
    /// which is final up to there.
    ///
    /// [`LiveProcessor`]: super::LiveProcessor
    Late {
        /// The input the event was meant for.
        key: Key,
        /// The instant the rejected event starts at.
        timestamp: Timestamp,
        /// The frontier of the input: events must not start before it.
        frontier: Timestamp,
    },

    /// A [`LiveProcessor`] was fed after `finish` closed it.
    ///
    /// [`LiveProcessor`]: super::LiveProcessor
    Finished,
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
            Self::ProjectionMismatch {
                key,
                path,
                expected,
                found,
            } => write!(
                f,
                "the field `{path}` of `{key}` cannot be read as {expected}, it is {found}"
            ),
            Self::SchemaMismatch { key, error } => {
                write!(
                    f,
                    "the result for `{key}` does not have its schema, {error}"
                )
            }
            Self::Cycle { keys } => {
                let keys: Vec<&str> = keys.iter().map(Key::as_str).collect();
                write!(
                    f,
                    "the outputs {} read each other in a circle",
                    keys.join(", ")
                )
            }
            Self::Late {
                key,
                timestamp,
                frontier,
            } => write!(
                f,
                "an event at {timestamp} is late for `{key}`, whose data is final up to {frontier}"
            ),
            Self::DuplicateMember { key } => {
                write!(f, "two members of the result would be filed under `{key}`")
            }
            Self::SeveralMembers => {
                f.write_str("an output per entity has one member per entity, not one sequence")
            }
            Self::GroupedRead { key } => write!(
                f,
                "a live processor cannot read `{key}`, a member of a per-entity output \
                 or the member a group is made of"
            ),
            Self::MissingField { key, path } => {
                write!(f, "the declared input `{key}` has no field `{path}`")
            }
            Self::FieldMismatch {
                key,
                path,
                expected,
                found,
            } => write!(
                f,
                "the field `{path}` of `{key}` is {expected}, but the condition compares it \
                 with {found}"
            ),
            Self::InputIsOutput { key } => {
                write!(f, "`{key}` is declared as an input and is also an output")
            }
            Self::TransformShape {
                key,
                expected,
                found,
            } => write!(
                f,
                "a transform of `{key}` must keep its shape, {expected}, but produced {found}"
            ),
            Self::InOutput { output, error } => write!(f, "in the output `{output}`: {error}"),
            Self::Finished => f.write_str("the live processor was finished"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InOutput { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}
