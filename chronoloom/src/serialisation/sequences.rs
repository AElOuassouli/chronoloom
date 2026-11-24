//! The two shapes of sequence, as a document of their own and as the body of a
//! member of a system.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::processing::Edge;
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::normalization::normalize;
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};

use super::{spans, Document, Format};

/// The body of a point sequence, with the document's own fields.
#[derive(Deserialize)]
pub(super) struct PointsBody<T> {
    attribute: Attribute,
    #[serde(default)]
    observed: Extent,
    events: Vec<TimePointEvent<T>>,
}

/// A point sequence borrowed for writing.
#[derive(Serialize)]
pub(super) struct PointsBodyRef<'a, T> {
    attribute: &'a Attribute,
    observed: &'a Extent,
    events: &'a [TimePointEvent<T>],
}

impl<'a, T> PointsBodyRef<'a, T> {
    pub(super) fn new(sequence: &'a TimePointSequence<T>) -> Self {
        Self {
            attribute: sequence.attribute(),
            observed: sequence.observed(),
            events: sequence.as_slice(),
        }
    }
}

impl<T> PointsBody<T> {
    /// The sequence, with its events sorted and checked against the extent.
    ///
    /// A live member passes its `frontier`: its extent must stop there, and
    /// only its data before the frontier is checked against the extent.
    pub(super) fn build(self, frontier: Option<Timestamp>) -> Result<TimePointSequence<T>, String> {
        let allowed = data_extent(&self.observed, frontier)?;
        self.assemble(&allowed, |_| false)
    }

    /// The sequence, its events sorted; `lenient` says which values may lie
    /// outside the extent.
    fn assemble(
        self,
        allowed: &Extent,
        lenient: impl Fn(&T) -> bool,
    ) -> Result<TimePointSequence<T>, String> {
        let mut events = self.events;
        events.sort_by_key(TimePointEvent::timestamp);
        if let Some(event) = events.iter().find(|event| {
            let at = event.timestamp();
            at != Timestamp::MAX && !allowed.contains(at) && !lenient(event.value())
        }) {
            return Err(format!(
                "the event at {} lies outside the extent the sequence was observed over",
                event.timestamp()
            ));
        }

        Ok(TimePointSequence::from_parts(
            self.attribute,
            events,
            self.observed,
        ))
    }
}

impl<T: 'static> PointsBody<T> {
    /// The sequence of a derived stream a live system holds: the `end` edge of
    /// a span may lie where nothing was observed, at the end of a run of the
    /// extent or past it.
    pub(super) fn build_held(self) -> Result<TimePointSequence<T>, String> {
        let allowed = data_extent(&self.observed, None)?;
        self.assemble(&allowed, is_end)
    }
}

/// The body of a timeline, with the document's own fields.
#[derive(Deserialize)]
pub(super) struct IntervalsBody {
    attribute: Attribute,
    #[serde(default)]
    transformation: (Timestamp, Timestamp),
    #[serde(default)]
    observed: Extent,
    #[serde(with = "spans")]
    spans: Vec<TimeIntervalEvent<()>>,
}

/// A timeline borrowed for writing.
#[derive(Serialize)]
pub(super) struct IntervalsBodyRef<'a> {
    attribute: &'a Attribute,
    transformation: (Timestamp, Timestamp),
    observed: &'a Extent,
    #[serde(with = "spans")]
    spans: &'a [TimeIntervalEvent<()>],
}

impl<'a> IntervalsBodyRef<'a> {
    pub(super) fn new(timeline: &'a TimeIntervalSequence) -> Self {
        Self {
            attribute: timeline.attribute(),
            transformation: timeline.transformation(),
            observed: timeline.observed(),
            spans: timeline.as_slice(),
        }
    }
}

impl IntervalsBody {
    /// The timeline, with its spans normalized and checked against the extent.
    pub(super) fn build(self, frontier: Option<Timestamp>) -> Result<TimeIntervalSequence, String> {
        let allowed = data_extent(&self.observed, frontier)?;
        self.assemble(&allowed)
    }

    fn assemble(self, allowed: &Extent) -> Result<TimeIntervalSequence, String> {
        let spans = normalize(self.spans);
        if let Some(span) = spans
            .iter()
            .find(|span| !allowed.covers(span.start(), span.end()))
        {
            return Err(format!(
                "the span [{}, {}) lies outside the extent the sequence was observed over",
                span.start(),
                span.end()
            ));
        }

        Ok(TimeIntervalSequence::from_parts(
            self.attribute,
            self.transformation,
            spans,
            self.observed,
        ))
    }
}

/// The instants the data must lie inside.
///
/// For a live member, what is recorded at or past its frontier is not final and
/// lies outside the extent, so the data may reach anywhere from the frontier on;
/// before it, the data lies inside the extent as for any sequence. Its extent
/// must stop at the frontier.
fn data_extent(observed: &Extent, frontier: Option<Timestamp>) -> Result<Extent, String> {
    let mut allowed = observed.clone();
    let Some(frontier) = frontier else {
        return Ok(allowed);
    };

    if observed.as_slice().iter().any(|run| run.end() > frontier) {
        return Err(format!(
            "the extent reaches past the frontier {frontier}, where a live member stops observing"
        ));
    }

    allowed.grow(Extent::of_range(&(frontier..)).as_slice());
    Ok(allowed)
}

impl<T: Serialize> Serialize for TimePointSequence<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: PointsBodyRef::new(self),
        }
        .serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for TimePointSequence<T> {
    /// Sorts the events, keeping the order of those at one instant, and refuses
    /// one outside the extent.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<PointsBody<T>>::deserialize(deserializer)?;

        body.build(None).map_err(de::Error::custom)
    }
}

impl Serialize for TimeIntervalSequence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: IntervalsBodyRef::new(self),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TimeIntervalSequence {
    /// Normalizes the spans, and refuses one outside the extent.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<IntervalsBody>::deserialize(deserializer)?;

        body.build(None).map_err(de::Error::custom)
    }
}

/// Whether the value is the `end` edge of a span.
fn is_end<T: 'static>(value: &T) -> bool {
    (value as &dyn core::any::Any)
        .downcast_ref::<Edge>()
        .is_some_and(|edge| *edge == Edge::End)
}
