//! Changing a system: which sequences it holds, and recording events into
//! them as they arrive.

use core::any::{self, Any};

use crate::primitives::{TimeIntervalEvent, TimePointEvent};
use crate::processing::Recipe;
use crate::sequences::{Extent, TimePointSequence};
use crate::values::{Schema, SchemaError, Value};

use super::{Key, Member, Metadata, RecordError, Sequence, SequenceSystem};
use crate::stream_system::update::Pending;
use crate::stream_system::Update;

impl SequenceSystem {
    /// Add a sequence under `key`, returning whichever sequence that key filed
    /// before.
    ///
    /// The member starts with empty metadata, and replacing a member drops the
    /// metadata of the old one.
    ///
    /// ```
    /// use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};
    /// use chronoloom::sequence_system::SequenceSystem;
    ///
    /// let mut system = SequenceSystem::new();
    /// assert!(system.insert("up", TimeIntervalSequence::new("up")).is_none());
    ///
    /// // Same key, other shape: the timeline is displaced.
    /// let displaced = system.insert("up", TimePointSequence::<bool>::new("up"));
    /// assert!(displaced.is_some_and(|old| old.as_time_interval().is_some()));
    /// assert!(system.points::<bool>("up").is_some());
    /// ```
    pub fn insert(
        &mut self,
        key: impl Into<Key>,
        sequence: impl Into<Sequence>,
    ) -> Option<Sequence> {
        let member = Member {
            sequence: sequence.into(),
            metadata: Metadata::new(),
            frontier: None,
            lineage: None,
            entity: None,
            schema: None,
        };

        self.members
            .insert(key.into(), member)
            .map(|replaced| replaced.sequence)
    }

    /// Add a sequence written by `output` under `key`, as [`insert`] does but
    /// keeping the output as the member's lineage.
    ///
    /// [`insert`]: SequenceSystem::insert
    pub(crate) fn insert_derived(
        &mut self,
        key: Key,
        sequence: Sequence,
        output: Recipe,
        entity: Option<Value>,
    ) {
        let derived = Member {
            sequence,
            metadata: Metadata::new(),
            frontier: None,
            lineage: Some(output),
            entity,
            schema: None,
        };
        self.members.insert(key, derived);
    }

    /// Drop the lineage of the member under `key`, because its data has been
    /// recorded into.
    fn forget_lineage(&mut self, key: &str) {
        if let Some(member) = self.members.get_mut(key) {
            member.lineage = None;
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
    /// See [`SequenceSystem`] for what a schema is checked against and kept
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

    /// Drop every sequence and its metadata, leaving the system empty. The
    /// metadata of the system as a whole stays.
    pub fn clear(&mut self) {
        self.members.clear();
    }

    /// Record a point event into the sequence for `key`.
    ///
    /// The sequence keeps itself in time order, so this is amortized constant
    /// for an event arriving after everything already recorded — the usual
    /// case for a stream.
    ///
    /// # Errors
    ///
    /// If no sequence is for `key`, if it is a timeline, or if its events
    /// carry another value type than `T`. Nothing is recorded in any of those
    /// cases.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    /// use chronoloom::sequence_system::{RecordError, SequenceSystem};
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert("temperature", TimePointSequence::<f64>::new("temperature"));
    ///
    /// system.record_point("temperature", TimePointEvent::new(10, 21.5_f64))?;
    ///
    /// // An integer literal is not an `f64`, so this is refused.
    /// let refused = system.record_point("temperature", TimePointEvent::new(11, 22));
    /// assert!(matches!(refused, Err(RecordError::ValueTypeMismatch { .. })));
    /// # Ok::<(), RecordError>(())
    /// ```
    pub fn record_point<T: 'static>(
        &mut self,
        key: &str,
        event: TimePointEvent<T>,
    ) -> Result<(), RecordError> {
        self.reject_late(key, Some(event.timestamp()))?;

        let member = self.members.get_mut(key).ok_or_else(|| unknown(key))?;
        let recorded = match &mut member.sequence {
            Sequence::TimePoint(points) => {
                let expected = points.value_type_name();
                let typed = points
                    .as_erased_mut()
                    .as_any_mut()
                    .downcast_mut::<TimePointSequence<T>>()
                    .ok_or_else(|| RecordError::ValueTypeMismatch {
                        key: key.to_owned(),
                        expected,
                        found: any::type_name::<T>(),
                    })?;

                let as_value: &dyn Any = &event;
                if let (Some(schema), Some(value)) = (
                    &member.schema,
                    as_value.downcast_ref::<TimePointEvent<Value>>(),
                ) {
                    schema
                        .check(value.value())
                        .map_err(|error| schema_mismatch(key, error))?;
                }

                typed.insert(event);
                member.lineage = None;
                member.entity = None;
                Ok(())
            }
            Sequence::TimeInterval(_) => Err(shape_mismatch(key)),
        };

        self.confine_to_frontier(key);
        recorded
    }

    /// Record a span into the timeline for `key`, merging it into what the
    /// timeline already covers.
    ///
    /// # Errors
    ///
    /// If no sequence is for `key`, or if it is a point sequence. Nothing is
    /// recorded in either case.
    pub fn record_span(
        &mut self,
        key: &str,
        span: TimeIntervalEvent<()>,
    ) -> Result<(), RecordError> {
        self.reject_late(key, Some(span.start()))?;

        let recorded = match self.member_mut(key)? {
            Sequence::TimeInterval(timeline) => {
                timeline.insert(span);
                self.forget_lineage(key);
                Ok(())
            }
            Sequence::TimePoint(_) => Err(shape_mismatch(key)),
        };

        self.confine_to_frontier(key);
        recorded
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
    /// The first push, in push order, that [`record_point`] or [`record_span`]
    /// would have refused, with the same error.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    /// use chronoloom::sequence_system::{RecordError, SequenceSystem};
    /// use chronoloom::stream_system::Update;
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert("temperature", TimePointSequence::<f64>::new("temperature"));
    ///
    /// let mut update = Update::new();
    /// update
    ///     .points("temperature", [TimePointEvent::new(10, 21.5_f64)])
    ///     .points("humidity", [TimePointEvent::new(10, 0.4_f64)]);
    ///
    /// // No sequence for `humidity`, so not even the temperature goes in.
    /// let refused = system.record_update(update);
    /// assert!(matches!(refused, Err(RecordError::Unknown { .. })));
    /// assert_eq!(system.points::<f64>("temperature").map(|s| s.len()), Some(0));
    /// ```
    ///
    /// [`record_point`]: SequenceSystem::record_point
    /// [`record_span`]: SequenceSystem::record_span
    pub fn record_update(&mut self, update: Update) -> Result<(), RecordError> {
        for (key, pending) in &update.chunks {
            self.check(key.as_str(), pending)?;
            self.reject_late(key.as_str(), pending.earliest())?;
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

            self.confine_to_frontier(key.as_str());
            self.forget_lineage(key.as_str());
        }

        Ok(())
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

impl SequenceSystem {
    /// Narrow every member's extent to its overlap with `extent`, cutting the
    /// data outside it.
    ///
    /// ```
    /// use chronoloom::primitives::TimeIntervalEvent;
    /// use chronoloom::sequences::{Extent, TimeIntervalSequence};
    /// use chronoloom::sequence_system::SequenceSystem;
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert("up", TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 50)?]));
    ///
    /// system.observe(&Extent::from_spans(vec![TimeIntervalEvent::span(0, 20)?]));
    ///
    /// let up = system.get("up").and_then(|member| member.as_time_interval()).unwrap();
    /// assert_eq!(up.active_duration(), 20);
    /// assert!(!up.is_observed(30));
    /// # Ok::<(), chronoloom::primitives::IntervalError>(())
    /// ```
    pub fn observe(&mut self, extent: &Extent) {
        for member in self.members.values_mut() {
            match &mut member.sequence {
                Sequence::TimePoint(points) => points.as_erased_mut().observe(extent),
                Sequence::TimeInterval(timeline) => timeline.observe(extent),
            }
        }
    }
}
