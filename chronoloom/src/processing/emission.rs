//! What a live processor hands to its host.

use crate::primitives::{TimePointEvent, Timestamp};
use crate::sequence_system::{AnyValue, Key};

use super::edge::Edge;

/// What a live processor emitted for one output, final and never taken back.
///
/// - [`Edge`](Emitted::Edge) for an output that is a state, or `Events::Edges`:
///   the start or the end of one of its spans.
/// - [`Point`](Emitted::Point) for an `Events::Matching`, `Events::Project` or
///   `Events::Changes` output: a point of the member it reads, or the field projected from one,
///   whose value type the processor does not know, so the value comes erased;
///   read it back with [`AnyValue::downcast_ref`], as the type the projection's
///   `Kind` stands for (`bool`, `i64`, `f64`, `String` or `Value`).
///
/// [`LiveProcessor`](super::LiveProcessor) says when an emission is final.
#[derive(Debug, Clone, PartialEq)]
pub enum Emitted {
    /// A span of the state starts or ends at the instant.
    Edge(TimePointEvent<Edge>),
    /// A point the output keeps, with the value of the point it was.
    Point(TimePointEvent<AnyValue>),
}

impl Emitted {
    /// The instant of the edge or point.
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        match self {
            Self::Edge(edge) => edge.timestamp(),
            Self::Point(point) => point.timestamp(),
        }
    }
}

/// One emission of a [`LiveProcessor`](super::LiveProcessor): which output it
/// belongs to, and what it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Emission {
    /// The key of the output the emission belongs to.
    pub key: Key,
    /// The edge or point.
    pub item: Emitted,
}

impl Emission {
    /// The instant of the emission.
    #[must_use]
    pub const fn timestamp(&self) -> Timestamp {
        self.item.timestamp()
    }
}
