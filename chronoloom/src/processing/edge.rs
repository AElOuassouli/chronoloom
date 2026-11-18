//! The two ways a point event can change a state.

/// What a point event does to a state: it starts it, or it ends it.
///
/// [`Events::Edges`] reads a state as a stream of these, a `Start` at the
/// first instant of each span and an `End` at the first instant after it,
/// which is the half-open end of the span. `Between` rebuilds the state from
/// such a stream.
///
/// [`Events::Edges`]: super::Events::Edges
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Edge {
    /// The state becomes active at this instant.
    Start,
    /// The state stops being active at this instant: the first instant that is
    /// no longer covered.
    End,
}
