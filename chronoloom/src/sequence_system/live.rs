//! Live members: members of a system that take events in time order and know
//! how far their data is final.

use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::sequences::extent::Extent;

use super::{Key, Member, Metadata, RecordError, Sequence, SequenceSystem};

impl SequenceSystem {
    /// Add a live sequence under `key`, whose data before `from` is final,
    /// returning whichever sequence that key filed before.
    ///
    /// The sequence is narrowed to what was observed before `from`, cutting
    /// the data from `from` on, and its frontier starts at `from`. The member
    /// starts with empty metadata, and replacing a member drops the old one's.
    pub fn insert_live(
        &mut self,
        key: impl Into<Key>,
        sequence: impl Into<Sequence>,
        from: Timestamp,
    ) -> Option<Sequence> {
        let mut sequence = sequence.into();
        let before = Extent::of_range(&(..from));
        match &mut sequence {
            Sequence::TimePoint(points) => points.as_erased_mut().observe(&before),
            Sequence::TimeInterval(timeline) => timeline.observe(&before),
        }

        let member = Member {
            sequence,
            metadata: Metadata::new(),
            frontier: Some(from),
            lineage: None,
            entity: None,
            schema: None,
        };

        self.members
            .insert(key.into(), member)
            .map(|replaced| replaced.sequence)
    }

    /// The frontier of the member for `key`, or `None` for a batch member or a
    /// key the system does not record.
    #[must_use]
    pub fn frontier_of(&self, key: &str) -> Option<Timestamp> {
        self.members.get(key)?.frontier
    }

    /// The earliest frontier among the live members, or `None` when there is
    /// no live member.
    #[must_use]
    pub fn frontier(&self) -> Option<Timestamp> {
        self.members
            .values()
            .filter_map(|member| member.frontier)
            .min()
    }

    /// Move the frontier of the member for `key` forward to `watermark`,
    /// observing everything in between.
    ///
    /// A watermark at or before the frontier changes nothing.
    ///
    /// # Errors
    ///
    /// [`RecordError::Unknown`] if no sequence is for `key`, and
    /// [`RecordError::NotLive`] if it is a batch member. Nothing changes in
    /// either case.
    pub fn advance(&mut self, key: &str, watermark: Timestamp) -> Result<(), RecordError> {
        self.move_frontier(key, watermark, true)
    }

    /// Move the frontier of the member for `key` forward to `watermark`
    /// without observing what lies in between, leaving a gap in its extent:
    /// an outage, where nothing is known. Data already recorded in that range
    /// is evidence its instants were seen, so the extent grows over that data
    /// alone.
    ///
    /// # Errors
    ///
    /// As [`advance`](SequenceSystem::advance).
    pub fn advance_unobserved(
        &mut self,
        key: &str,
        watermark: Timestamp,
    ) -> Result<(), RecordError> {
        self.move_frontier(key, watermark, false)
    }

    /// Advance every live member to `watermark`, as [`advance`] does.
    ///
    /// Batch members are left alone.
    ///
    /// [`advance`]: SequenceSystem::advance
    pub fn advance_all(&mut self, watermark: Timestamp) {
        for member in self.members.values_mut() {
            member.move_frontier(watermark, true);
        }
    }

    fn move_frontier(
        &mut self,
        key: &str,
        watermark: Timestamp,
        observed: bool,
    ) -> Result<(), RecordError> {
        let member = self
            .members
            .get_mut(key)
            .ok_or_else(|| RecordError::Unknown {
                key: key.to_owned(),
            })?;
        if member.frontier.is_none() {
            return Err(RecordError::NotLive {
                key: key.to_owned(),
            });
        }

        member.move_frontier(watermark, observed);
        Ok(())
    }

    /// Whether an event starting at `earliest` is late for the member for
    /// `key`. A batch member, an unknown key and no event are never late.
    pub(super) fn reject_late(
        &self,
        key: &str,
        earliest: Option<Timestamp>,
    ) -> Result<(), RecordError> {
        let frontier = self.members.get(key).and_then(|member| member.frontier);

        match (earliest, frontier) {
            (Some(timestamp), Some(frontier)) if timestamp < frontier => Err(RecordError::Late {
                key: key.to_owned(),
                timestamp,
                frontier,
            }),
            _ => Ok(()),
        }
    }

    /// Take back what recording grew the extent of a live member by: the
    /// extent is what was observed, and the frontier says how far that is.
    pub(super) fn confine_to_frontier(&mut self, key: &str) {
        if let Some(member) = self.members.get_mut(key) {
            member.confine_to_frontier();
        }
    }
}

impl Member {
    pub(super) fn observed_mut(&mut self) -> &mut Extent {
        match &mut self.sequence {
            Sequence::TimePoint(points) => points.as_erased_mut().observed_mut(),
            Sequence::TimeInterval(timeline) => timeline.observed_mut(),
        }
    }

    /// Move the frontier forward, if the member is live and `watermark` is
    /// past it.
    fn move_frontier(&mut self, watermark: Timestamp, observed: bool) {
        let Some(frontier) = self.frontier else {
            return;
        };
        if watermark <= frontier {
            return;
        }

        let seen = if observed {
            vec![TimeIntervalEvent::raw(frontier, watermark)]
        } else {
            // An outage observes nothing, but data already recorded in the
            // range it passes over is evidence its instants were seen: the
            // extent grows over that data alone, so the data becomes final
            // inside its extent, as for a batch member.
            self.data_between(frontier, watermark)
        };
        self.observed_mut().grow(&seen);
        self.frontier = Some(watermark);
    }

    /// The instants in `[start, end)` that the member's data covers, as
    /// normalized spans.
    fn data_between(&self, start: Timestamp, end: Timestamp) -> Vec<TimeIntervalEvent<()>> {
        let window = Extent::of_range(&(start..end));
        match &self.sequence {
            Sequence::TimeInterval(timeline) => {
                Extent::from_normalized(timeline.as_slice().to_vec()).intersection_spans(&window)
            }
            Sequence::TimePoint(points) => {
                let mut spans: Vec<TimeIntervalEvent<()>> = Vec::new();
                for instant in points.instants_in(start, end) {
                    let Some(next) = instant.checked_add(1) else {
                        continue;
                    };
                    match spans.last_mut() {
                        Some(last) if last.end() == instant => {
                            *last = TimeIntervalEvent::raw(last.start(), next);
                        }
                        _ => spans.push(TimeIntervalEvent::raw(instant, next)),
                    }
                }
                spans
            }
        }
    }

    fn confine_to_frontier(&mut self) {
        if let Some(frontier) = self.frontier {
            self.observed_mut().narrow(&Extent::of_range(&(..frontier)));
        }
    }
}
