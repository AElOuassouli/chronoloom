//! A [`LiveProcessor`] as a document: a checkpoint of its state.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::Timestamp;
use crate::processing::{Checkpoint, LiveProcessor, Untyped};
use crate::sequence_system::Key;

use super::sequences::{PointsBody, PointsBodyRef};
use super::{member_error, Document, Format};

#[derive(Serialize)]
struct UntypedRef<'a> {
    key: &'a Key,
    frontier: Timestamp,
    sequence: PointsBodyRef<'a, ()>,
}

#[derive(Deserialize)]
struct UntypedBody {
    key: Key,
    frontier: Timestamp,
    sequence: PointsBody<()>,
}

impl Serialize for Untyped {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        UntypedRef {
            key: &self.key,
            frontier: self.frontier,
            sequence: PointsBodyRef::new(&self.sequence),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Untyped {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let UntypedBody {
            key,
            frontier,
            sequence,
        } = UntypedBody::deserialize(deserializer)?;
        let sequence = sequence
            .build(Some(frontier))
            .map_err(|problem| de::Error::custom(member_error(&key, problem)))?;

        Ok(Self {
            key,
            frontier,
            sequence,
        })
    }
}

impl Serialize for LiveProcessor {
    /// # Errors
    ///
    /// Refuses a processor with a pattern on a value type outside the closed
    /// set, and an input whose value type is outside it, naming the key.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: self.checkpoint(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LiveProcessor {
    /// Rebuilds the state a processor does not determine, and checks it.
    ///
    /// # Errors
    ///
    /// Refuses a checkpoint whose inputs are not the ones its processors read,
    /// whose entities, cursors or views contradict each other or the
    /// frontiers, or whose processors do not pass [`Processor::check`] or
    /// cannot run online.
    ///
    /// [`Processor::check`]: crate::Processor::check
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<Checkpoint>::deserialize(deserializer)?;

        Self::from_checkpoint(body).map_err(de::Error::custom)
    }
}
