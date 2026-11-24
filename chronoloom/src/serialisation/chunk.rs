//! A [`Chunk`] as data: the range it covers and its sequence, tagged as the
//! members of a system are. Chunks are what the running state of a live
//! system holds back between steps.

use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};

use crate::operator::Chunk;
use crate::primitives::Timestamp;
use crate::sequence_system::{Key, Sequence};

use super::stream_system::{SequenceBody, SequenceRef, SUPPORTED};

#[derive(Serialize)]
#[serde(rename = "Chunk")]
struct ChunkRef<'a> {
    from: Timestamp,
    to: Timestamp,
    sequence: SequenceRef<'a>,
}

impl Serialize for Chunk {
    /// # Errors
    ///
    /// Refuses a chunk of points whose value type is outside the closed set.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let range = self.range();
        ChunkRef {
            from: range.start,
            to: range.end,
            sequence: SequenceRef::of(&Key::from("chunk"), self.sequence())
                .map_err(ser::Error::custom)?,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
struct ChunkBody {
    from: Timestamp,
    to: Timestamp,
    sequence: SequenceBody,
}

impl<'de> Deserialize<'de> for Chunk {
    /// # Errors
    ///
    /// Refuses a chunk that ends before it starts, whose sequence breaks an
    /// invariant or holds values of a type outside the closed set, or whose
    /// spans, points or extent go outside `[from, to)` (a point at
    /// `Timestamp::MAX` excepted when `to` is the end of time).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let ChunkBody { from, to, sequence } = ChunkBody::deserialize(deserializer)?;
        if to < from {
            return Err(de::Error::custom(format_args!(
                "a chunk ends at {to}, before it starts at {from}"
            )));
        }
        let sequence = sequence
            .build()
            .map_err(de::Error::custom)?
            .ok_or_else(|| {
                de::Error::custom(format_args!(
                    "a chunk holds values of a type the format does not support: it supports \
                     {SUPPORTED}"
                ))
            })?;
        within(&sequence, from, to).map_err(de::Error::custom)?;
        Ok(Self::new(from, to, sequence))
    }
}

/// Whether everything the sequence holds, and its extent, lies in `[from, to)`.
fn within(sequence: &Sequence, from: Timestamp, to: Timestamp) -> Result<(), String> {
    let outside = |start: Timestamp, end: Timestamp| start < from || end > to;
    if sequence
        .observed()
        .as_slice()
        .iter()
        .any(|run| outside(run.start(), run.end()))
    {
        return Err(format!(
            "the extent of a chunk over [{from}, {to}) goes outside it"
        ));
    }
    let strays = match sequence {
        Sequence::TimePoint(points) => points
            .instants_in(Timestamp::MIN, Timestamp::MAX)
            .into_iter()
            .any(|at| at < from || at >= to),
        Sequence::TimeInterval(timeline) => timeline
            .iter()
            .any(|span| outside(span.start(), span.end())),
    };
    if strays {
        return Err(format!("a chunk over [{from}, {to}) holds data outside it"));
    }
    Ok(())
}
