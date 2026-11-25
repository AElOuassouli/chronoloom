//! Changing a system: which sequences it holds, and recording events into
//! them as they arrive.

use core::any;
use core::ops::Bound;

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::sequences::Extent;
use crate::values::{Schema, SchemaError, Value};

use super::erased::ErasedPoints;
use super::{Key, Member, Metadata, RecordError, Sequence, Store};
use crate::stream_system::update::Pending;
use crate::stream_system::Update;

impl Store {
    /// Add a sequence under `key`, returning whichever sequence that key filed
    /// before.
    ///
    /// The member starts with empty metadata, and replacing a member drops the
    /// metadata of the old one.
    pub fn insert(
        &mut self,
        key: impl Into<Key>,
        sequence: impl Into<Sequence>,
    ) -> Option<Sequence> {
        let member = Member {
            sequence: sequence.into(),
            metadata: Metadata::new(),
            derived: false,
            entity: None,
            schema: None,
        };

        self.members
            .insert(key.into(), member)
            .map(|replaced| replaced.sequence)
    }

    /// Add a sequence written by a processor under `key`, as [`insert`] does
    /// but marking the member as derived.
    ///
    /// [`insert`]: Store::insert
    pub(crate) fn insert_derived(&mut self, key: Key, sequence: Sequence, entity: Option<Value>) {
        let derived = Member {
            sequence,
            metadata: Metadata::new(),
            derived: true,
            entity,
            schema: None,
        };
        self.members.insert(key, derived);
    }

    /// Mark the member under `key` as no longer derived, because its data has
    /// been recorded into.
    fn forget_derivation(&mut self, key: &str) {
        if let Some(member) = self.members.get_mut(key) {
            member.derived = false;
            member.entity = None;
        }
    }

    /// Replace the attribute of the sequence for `key`, keeping its key and its
    /// data.
    ///
    /// A timeline's [`transformation`] is reset, as [`set_attribute`] does.
    ///
    /// # Errors
    ///
    /// [`RecordError::Unknown`] if no sequence is for `key`; nothing changes.
    ///
    /// [`set_attribute`]: crate::sequences::TimeIntervalSequence::set_attribute
    /// [`transformation`]: crate::sequences::TimeIntervalSequence::transformation
    pub fn relabel(&mut self, key: &str, attribute: impl Into<String>) -> Result<(), RecordError> {
        match self.member_mut(key)? {
            Sequence::TimePoint(points) => points.set_attribute(attribute.into()),
            Sequence::TimeInterval(timeline) => timeline.set_attribute(attribute),
        }

        Ok(())
    }

    /// Require every value of the member for `key` to have the shape `schema`,
    /// from now on, replacing the schema it had.
    ///
    /// See [`Store`] for what a schema is checked against and kept
    /// through.
    ///
    /// # Errors
    ///
    /// [`RecordError::Unknown`] if no sequence is for `key`,
    /// [`RecordError::ShapeMismatch`] if it is a timeline,
    /// [`RecordError::ValueTypeMismatch`] if its points carry values other than
    /// [`Value`], and [`RecordError::SchemaMismatch`] naming the first recorded
    /// event, in time order, that does not have the shape. Nothing changes in
    /// any of those cases.
    pub fn set_schema(&mut self, key: &str, schema: Schema) -> Result<(), RecordError> {
        let member = self.members.get_mut(key).ok_or_else(|| unknown(key))?;
        match &member.sequence {
            Sequence::TimePoint(points) => {
                let values = points.downcast_ref::<Value>().ok_or_else(|| {
                    RecordError::ValueTypeMismatch {
                        key: key.to_owned(),
                        expected: points.value_type_name(),
                        found: any::type_name::<Value>(),
                    }
                })?;
                if let Some(error) = values
                    .iter()
                    .find_map(|event| schema.check(event.value()).err())
                {
                    return Err(schema_mismatch(key, error));
                }
            }
            Sequence::TimeInterval(_) => return Err(shape_mismatch(key)),
        }

        member.schema = Some(schema);
        Ok(())
    }

    /// Take the sequence for `key` out of the system, dropping its metadata.
    pub fn remove(&mut self, key: &str) -> Option<Sequence> {
        self.members.remove(key).map(|member| member.sequence)
    }

    /// Record every event in `update`, all or nothing.
    ///
    /// Every push in the update is checked against its sequence before anything
    /// is recorded, so an update that fails leaves the system exactly as it was.
    /// Each push then goes in through its sequence's `Extend`, which reorders
    /// only when the arrivals broke the time order — so an update of in-order
    /// events costs no sorting, and is cheaper than recording them one by one.
    ///
    /// # Errors
    ///
    /// The first push, in push order, that [`record_point`] or [`record_span`] would have refused,
    /// with the same error.
    ///
    /// [`record_point`]: Store::record_point
    /// [`record_span`]: Store::record_span
    pub fn record_update(&mut self, update: Update) -> Result<(), RecordError> {
        for (key, pending) in &update.chunks {
            self.check(key.as_str(), pending)?;
        }

        for (key, pending) in update.chunks {
            match (
                self.member_mut(key.as_str()).expect("checked above"),
                pending,
            ) {
                (Sequence::TimePoint(points), Pending::Points(events)) => {
                    events.record_into(points.as_erased_mut());
                }
                (Sequence::TimeInterval(timeline), Pending::Spans(spans)) => timeline.extend(spans),
                _ => unreachable!("every push was checked against its sequence above"),
            }

            self.forget_derivation(key.as_str());
        }

        Ok(())
    }

    /// Append `next`, the sequence over the range that follows what the
    /// derived member under `key` holds, in place: its spans or points are
    /// added, and its extent grows over the extent of `next`. The member keeps
    /// its metadata.
    ///
    /// # Panics
    ///
    /// If no member is under `key`, or `next` is of another shape or value type
    /// than the member: a live stream system gives chunks of one shape.
    pub(crate) fn append_derived(&mut self, key: &str, next: &Sequence) {
        let held = self
            .member_mut(key)
            .expect("a derived member is held before it is appended to");
        match (held, next) {
            (Sequence::TimeInterval(held), Sequence::TimeInterval(next)) => {
                for span in next.iter() {
                    held.insert(*span);
                }
                held.observed_mut().grow(next.observed().as_slice());
            }
            (Sequence::TimePoint(held), Sequence::TimePoint(next)) => {
                assert!(
                    held.append(next),
                    "chunks of one output carry one value type"
                );
            }
            _ => panic!("chunks of one output have one shape"),
        }
    }

    /// Whether `pending` could be recorded into the sequence for `key`.
    fn check(&self, key: &str, pending: &Pending) -> Result<(), RecordError> {
        let member = self.members.get(key).ok_or_else(|| unknown(key))?;

        match (&member.sequence, pending) {
            (Sequence::TimePoint(points), Pending::Points(events)) => {
                if !events.fits(&*points.0) {
                    return Err(RecordError::ValueTypeMismatch {
                        key: key.to_owned(),
                        expected: points.value_type_name(),
                        found: events.value_type_name(),
                    });
                }

                match member
                    .schema
                    .as_ref()
                    .and_then(|schema| events.first_violation(schema))
                {
                    Some(error) => Err(schema_mismatch(key, error)),
                    None => Ok(()),
                }
            }
            (Sequence::TimeInterval(_), Pending::Spans(_)) => Ok(()),
            _ => Err(shape_mismatch(key)),
        }
    }

    /// The sequence for `key`, for recording into.
    fn member_mut(&mut self, key: &str) -> Result<&mut Sequence, RecordError> {
        self.members
            .get_mut(key)
            .map(|member| &mut member.sequence)
            .ok_or_else(|| unknown(key))
    }
}

fn unknown(key: &str) -> RecordError {
    RecordError::Unknown {
        key: key.to_owned(),
    }
}

fn schema_mismatch(key: &str, error: SchemaError) -> RecordError {
    RecordError::SchemaMismatch {
        key: key.to_owned(),
        error,
    }
}

fn shape_mismatch(key: &str) -> RecordError {
    RecordError::ShapeMismatch {
        key: key.to_owned(),
    }
}

impl Store {
    /// Narrow every member's extent to its overlap with `extent`, cutting the
    /// data outside it.
    pub fn observe(&mut self, extent: &Extent) {
        for member in self.members.values_mut() {
            match &mut member.sequence {
                Sequence::TimePoint(points) => points.as_erased_mut().observe(extent),
                Sequence::TimeInterval(timeline) => timeline.observe(extent),
            }
        }
    }
}

/// What a member held from an instant on, with its extent: enough to put the
/// member back as it was after something was recorded or appended at or after
/// that instant, at the cost of what lies there and not of the whole member.
#[derive(Debug)]
pub(crate) struct Tail {
    from: Timestamp,
    observed: Extent,
    data: TailData,
}

#[derive(Debug)]
enum TailData {
    Points(Box<dyn ErasedPoints>),
    Spans(Vec<TimeIntervalEvent<()>>),
}

impl Store {
    /// The part of the member under `key` that recording at or after `from`
    /// may change. `None` if no member is under `key`.
    pub(crate) fn tail_from(&self, key: &str, from: Timestamp) -> Option<Tail> {
        let member = self.members.get(key)?;
        let (observed, data) = match &member.sequence {
            Sequence::TimePoint(points) => (
                points.observed().clone(),
                TailData::Points(
                    points
                        .0
                        .slice_bounds((Bound::Included(from), Bound::Unbounded)),
                ),
            ),
            Sequence::TimeInterval(timeline) => (
                timeline.observed().clone(),
                TailData::Spans(timeline.tail_from(from)),
            ),
        };
        Some(Tail {
            from,
            observed,
            data,
        })
    }

    /// Put the member under `key` back as `tail` took it, whatever was
    /// recorded or appended at or after its instant since. Nothing changes if
    /// no member is under `key` or it is of another shape.
    pub(crate) fn restore_tail(&mut self, key: &str, tail: Tail) {
        let Some(member) = self.members.get_mut(key) else {
            return;
        };
        let Tail {
            from,
            observed,
            data,
        } = tail;
        match (&mut member.sequence, data) {
            (Sequence::TimePoint(points), TailData::Points(tail)) => {
                assert!(
                    points.as_erased_mut().restore_tail(from, &*tail, observed),
                    "a tail is put back into the member it was taken from"
                );
            }
            (Sequence::TimeInterval(timeline), TailData::Spans(spans)) => {
                timeline.restore_tail(from, spans, observed);
            }
            _ => {}
        }
    }
}
