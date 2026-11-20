//! Extents, spans and the events that carry a value.

use serde::{de, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::sequences::Extent;

/// Spans as a list of `[start, end]` pairs, for `#[serde(with = ..)]`.
///
/// Reading refuses a pair with `start >= end` and leaves the order and the
/// overlaps to the caller, which normalizes.
pub(crate) mod spans {
    use super::{de, Deserialize, Deserializer, Serializer, TimeIntervalEvent, Timestamp};

    pub(crate) fn serialize<S: Serializer>(
        spans: &[TimeIntervalEvent<()>],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(spans.iter().map(TimeIntervalEvent::bounds))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<TimeIntervalEvent<()>>, D::Error> {
        Vec::<(Timestamp, Timestamp)>::deserialize(deserializer)?
            .into_iter()
            .map(|(start, end)| TimeIntervalEvent::span(start, end).map_err(de::Error::custom))
            .collect()
    }
}

impl Serialize for Extent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        spans::serialize(self.as_slice(), serializer)
    }
}

impl<'de> Deserialize<'de> for Extent {
    /// Normalizes the spans: sorted, with those that overlap or touch merged.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        spans::deserialize(deserializer).map(Self::from_spans)
    }
}

impl<T: Serialize> Serialize for TimeIntervalEvent<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut event = serializer.serialize_struct("TimeIntervalEvent", 3)?;
        event.serialize_field("start", &self.start())?;
        event.serialize_field("end", &self.end())?;
        event.serialize_field("value", self.value())?;
        event.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for TimeIntervalEvent<T> {
    /// Refuses an event whose `end` is not after its `start`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "TimeIntervalEvent", deny_unknown_fields)]
        struct Fields<T> {
            start: Timestamp,
            end: Timestamp,
            value: T,
        }

        let Fields { start, end, value } = Fields::deserialize(deserializer)?;

        Self::new(start, end, value).map_err(de::Error::custom)
    }
}
