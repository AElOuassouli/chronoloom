//! What every mode of a stream system shares: reading, metadata, the list of
//! processors, and evaluating the processors a change affects.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeBounds;

use crate::primitives::Timestamp;
use crate::sequence_system::{Key, Metadata, Sequence, SequenceSystem};
use crate::sequences::{TimeIntervalSequence, TimePointSequence};
use crate::values::Schema;

use crate::processor::Processor;

use super::{Batch, Mode, Report, StreamSystem, UpdateError};

impl<M: Mode> StreamSystem<M> {
    /// The stream or derived stream under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Sequence> {
        self.members.get(key)
    }

    /// The point sequence under `key`, if it holds `T` values.
    #[must_use]
    pub fn points<T: 'static>(&self, key: &str) -> Option<&TimePointSequence<T>> {
        self.members.points(key)
    }

    /// The timeline under `key`.
    #[must_use]
    pub fn intervals(&self, key: &str) -> Option<&TimeIntervalSequence> {
        self.members.intervals(key)
    }

    /// Whether the system holds a stream or a derived stream under `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.members.contains(key)
    }

    /// Every key, streams and derived streams, in key order.
    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.members.keys()
    }

    /// The keys of the streams, in key order.
    pub fn streams(&self) -> impl Iterator<Item = &Key> {
        self.members
            .keys()
            .filter(|key| !self.is_derived(key.as_str()))
    }

    /// The keys of the derived streams, in key order.
    pub fn derived(&self) -> impl Iterator<Item = &Key> {
        self.members
            .keys()
            .filter(|key| self.is_derived(key.as_str()))
    }

    /// Whether `key` is a derived stream.
    #[must_use]
    pub fn is_derived(&self, key: &str) -> bool {
        self.producer(key).is_some()
    }

    /// The processor that produces the derived stream under `key`.
    #[must_use]
    pub fn producer(&self, key: &str) -> Option<&Processor> {
        let (output, _) = self
            .produced
            .iter()
            .find(|(_, members)| members.contains(key))?;
        self.processors
            .iter()
            .find(|processor| processor.output() == output)
    }

    /// The entity the derived stream under `key` is about, for a member of a
    /// per-entity output.
    #[must_use]
    pub fn entity(&self, key: &str) -> Option<&crate::values::Value> {
        self.members.lineage_entity(key)
    }

    /// The processors, in the order they are evaluated in.
    #[must_use]
    pub fn processors(&self) -> &[Processor] {
        &self.processors
    }

    /// The metadata of the system itself.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        self.members.metadata()
    }

    /// The metadata of the system itself, to change.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        self.members.metadata_mut()
    }

    /// The metadata of the stream or derived stream under `key`. A derived
    /// stream keeps its metadata when it is evaluated again.
    #[must_use]
    pub fn member_metadata(&self, key: &str) -> Option<&Metadata> {
        self.members.member_metadata(key)
    }

    /// The metadata of the stream or derived stream under `key`, to change.
    pub fn member_metadata_mut(&mut self, key: &str) -> Option<&mut Metadata> {
        self.members.member_metadata_mut(key)
    }

    /// The schema of the stream under `key`, if it has one.
    #[must_use]
    pub fn schema(&self, key: &str) -> Option<&Schema> {
        self.members.schema(key)
    }

    /// The system cut to `range`, in batch mode: every stream and every
    /// derived stream sliced, keys, labels, metadata and processors kept.
    ///
    /// A derived stream is the whole system's, cut: what the full data says
    /// inside the window, such as a presence that began before it. That can
    /// differ from evaluating the processors over the cut streams alone, and
    /// a later change to the slice evaluates them over its own streams.
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<Timestamp>) -> StreamSystem<Batch> {
        StreamSystem {
            members: self.members.slice(range),
            processors: self.processors.clone(),
            produced: self.produced.clone(),
            mode: Batch,
        }
    }

    /// The output key of a processor that reads `key`, if any does.
    pub(super) fn reader_of(&self, key: &Key) -> Option<&Key> {
        self.processors
            .iter()
            .find(|processor| processor.inputs().contains(key))
            .map(Processor::output)
    }
}

/// Evaluate again the processors that read a key of `changed`, directly or
/// through the output of another, in the order of `processors`, or all of
/// them when `changed` is `None`. Each processor's members in `members`
/// replace the ones it wrote before, keeping their metadata. Returns the
/// derived streams whose content changed.
///
/// On an error, `members` and `produced` may be half updated: callers work on
/// copies.
pub(super) fn evaluate(
    processors: &[Processor],
    members: &mut SequenceSystem,
    produced: &mut BTreeMap<Key, BTreeSet<Key>>,
    changed: Option<BTreeSet<Key>>,
) -> Result<Report, UpdateError> {
    let mut changed = changed;
    let mut report = Report::default();

    for processor in processors {
        if let Some(changed) = &changed {
            if !processor.inputs().iter().any(|key| changed.contains(*key)) {
                continue;
            }
        }

        let output = processor.output();
        let mut fresh = processor
            .as_set()
            .run(members)
            .map_err(|error| UpdateError::Process {
                processor: output.clone(),
                error,
            })?;

        let before = produced.remove(output).unwrap_or_default();
        let after: BTreeSet<Key> = fresh.keys().cloned().collect();
        if let Some(key) = after
            .iter()
            .find(|key| !before.contains(*key) && members.contains(key.as_str()))
        {
            return Err(UpdateError::Taken { key: key.clone() });
        }

        for key in before.difference(&after) {
            members.remove(key.as_str());
            report.changed.insert(key.clone());
        }
        for key in &after {
            let lineage = fresh
                .lineage(key.as_str())
                .cloned()
                .expect("a processor's run records its output as lineage");
            let entity = fresh.lineage_entity(key.as_str()).cloned();
            let sequence = fresh
                .remove(key.as_str())
                .expect("every key listed is held");
            if members.get(key.as_str()) != Some(&sequence) {
                report.changed.insert(key.clone());
            }

            let metadata = members.member_metadata(key.as_str()).cloned();
            members.insert_derived(key.clone(), sequence, lineage, entity);
            if let (Some(metadata), Some(kept)) =
                (metadata, members.member_metadata_mut(key.as_str()))
            {
                *kept = metadata;
            }
        }

        if let Some(changed) = &mut changed {
            changed.insert(output.clone());
            changed.extend(before.union(&after).cloned());
        }
        produced.insert(output.clone(), after);
    }

    Ok(report)
}
