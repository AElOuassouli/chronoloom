//! Changing a system: which sequences it holds, and recording events into
//! them as they arrive.

use core::any;

use crate::primitives::{TimeIntervalEvent, TimePointEvent};
use crate::sequences::TimePointSequence;

use super::batch::{Batch, Pending};
use super::{RecordError, Sequence, SequenceSystem};

impl SequenceSystem {
    /// Add a sequence under its attribute, returning whichever sequence that
    /// attribute filed before.
    ///
    /// ```
    /// use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};
    /// use chronoloom::sequence_system::SequenceSystem;
    ///
    /// let mut system = SequenceSystem::new();
    /// assert!(system.insert(TimeIntervalSequence::new("up")).is_none());
    ///
    /// // Same attribute, other shape: the timeline is displaced.
    /// let displaced = system.insert(TimePointSequence::<bool>::new("up"));
    /// assert!(displaced.is_some_and(|old| old.as_time_interval().is_some()));
    /// assert!(system.points::<bool>("up").is_some());
    /// ```
    pub fn insert(&mut self, sequence: impl Into<Sequence>) -> Option<Sequence> {
        let sequence = sequence.into();

        self.sequences.insert(sequence.label(), sequence)
    }

    /// Take the sequence for `attribute` out of the system.
    pub fn remove(&mut self, attribute: &str) -> Option<Sequence> {
        self.sequences.remove(attribute)
    }

    /// Drop every sequence, leaving the system empty.
    pub fn clear(&mut self) {
        self.sequences.clear();
    }

    /// Record a point event into the sequence for `attribute`.
    ///
    /// The sequence keeps itself in time order, so this is amortized constant
    /// for an event arriving after everything already recorded — the usual
    /// case for a stream.
    ///
    /// # Errors
    ///
    /// If no sequence is for `attribute`, if it is a timeline, or if its events
    /// carry another value type than `T`. Nothing is recorded in any of those
    /// cases.
    ///
    /// ```
    /// use chronoloom::primitives::TimePointEvent;
    /// use chronoloom::sequences::TimePointSequence;
    /// use chronoloom::sequence_system::{RecordError, SequenceSystem};
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert(TimePointSequence::<f64>::new("temperature"));
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
        attribute: &str,
        event: TimePointEvent<T>,
    ) -> Result<(), RecordError> {
        match self.member_mut(attribute)? {
            Sequence::TimePoint(points) => {
                let expected = points.value_type_name();
                let typed = points
                    .as_erased_mut()
                    .as_any_mut()
                    .downcast_mut::<TimePointSequence<T>>()
                    .ok_or_else(|| RecordError::ValueTypeMismatch {
                        attribute: attribute.to_owned(),
                        expected,
                        found: any::type_name::<T>(),
                    })?;

                typed.insert(event);
                Ok(())
            }
            Sequence::TimeInterval(_) => Err(shape_mismatch(attribute)),
        }
    }

    /// Record a span into the timeline for `attribute`, merging it into what the
    /// timeline already covers.
    ///
    /// # Errors
    ///
    /// If no sequence is for `attribute`, or if it is a point sequence. Nothing is
    /// recorded in either case.
    pub fn record_span(
        &mut self,
        attribute: &str,
        span: TimeIntervalEvent<()>,
    ) -> Result<(), RecordError> {
        match self.member_mut(attribute)? {
            Sequence::TimeInterval(timeline) => {
                timeline.insert(span);
                Ok(())
            }
            Sequence::TimePoint(_) => Err(shape_mismatch(attribute)),
        }
    }

    /// Record every event in `batch`, all or nothing.
    ///
    /// Every push in the batch is checked against its sequence before anything
    /// is recorded, so a batch that fails leaves the system exactly as it was.
    /// Each push then goes in through its sequence's `Extend`, which reorders
    /// only when the arrivals broke the time order — so a batch of in-order
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
    /// use chronoloom::sequence_system::{Batch, RecordError, SequenceSystem};
    ///
    /// let mut system = SequenceSystem::new();
    /// system.insert(TimePointSequence::<f64>::new("temperature"));
    ///
    /// let mut batch = Batch::new();
    /// batch
    ///     .points("temperature", [TimePointEvent::new(10, 21.5_f64)])
    ///     .points("humidity", [TimePointEvent::new(10, 0.4_f64)]);
    ///
    /// // No sequence for `humidity`, so not even the temperature goes in.
    /// let refused = system.record_batch(batch);
    /// assert!(matches!(refused, Err(RecordError::Unknown { .. })));
    /// assert_eq!(system.points::<f64>("temperature").map(|s| s.len()), Some(0));
    /// ```
    ///
    /// [`record_point`]: SequenceSystem::record_point
    /// [`record_span`]: SequenceSystem::record_span
    pub fn record_batch(&mut self, batch: Batch) -> Result<(), RecordError> {
        for (attribute, pending) in &batch.chunks {
            self.check(attribute, pending)?;
        }

        for (attribute, pending) in batch.chunks {
            match (self.member_mut(&attribute).expect("checked above"), pending) {
                (Sequence::TimePoint(points), Pending::Points(events)) => {
                    events.record_into(points.as_erased_mut());
                }
                (Sequence::TimeInterval(timeline), Pending::Spans(spans)) => timeline.extend(spans),
                _ => unreachable!("every push was checked against its sequence above"),
            }
        }

        Ok(())
    }

    /// Whether `pending` could be recorded into the sequence for `attribute`.
    fn check(&self, attribute: &str, pending: &Pending) -> Result<(), RecordError> {
        let member = self
            .sequences
            .get(attribute)
            .ok_or_else(|| unknown(attribute))?;

        match (member, pending) {
            (Sequence::TimePoint(points), Pending::Points(events)) => {
                if events.fits(&*points.0) {
                    Ok(())
                } else {
                    Err(RecordError::ValueTypeMismatch {
                        attribute: attribute.to_owned(),
                        expected: points.value_type_name(),
                        found: events.value_type_name(),
                    })
                }
            }
            (Sequence::TimeInterval(_), Pending::Spans(_)) => Ok(()),
            _ => Err(shape_mismatch(attribute)),
        }
    }

    /// The sequence for `attribute`, for recording into.
    fn member_mut(&mut self, attribute: &str) -> Result<&mut Sequence, RecordError> {
        self.sequences
            .get_mut(attribute)
            .ok_or_else(|| unknown(attribute))
    }
}

fn unknown(attribute: &str) -> RecordError {
    RecordError::Unknown {
        attribute: attribute.to_owned(),
    }
}

fn shape_mismatch(attribute: &str) -> RecordError {
    RecordError::ShapeMismatch {
        attribute: attribute.to_owned(),
    }
}
