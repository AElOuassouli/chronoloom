//! What every mode of a stream system shares: reading, metadata, the list of
//! processors, and evaluating the processors a change affects.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeBounds;

use crate::primitives::Timestamp;
use crate::sequences::{TimeIntervalSequence, TimePointSequence};
use crate::store::{Key, Metadata, Sequence, Store};
use crate::values::Schema;

use crate::processing::ProcessError;
use crate::processor::Processor;

use super::pipeline::{writes, Pipeline};
use super::{Batch, Mode, Report, StreamSystem, UpdateError};

impl<M: Mode> StreamSystem<M> {
    /// The stream or derived stream under `key`; for a transformed stream, its
    /// last version. [`raw`](StreamSystem::raw) gives the recorded data.
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

    /// The data recorded into the stream under `key`, before any transform:
    /// what [`get`](StreamSystem::get) gives for a stream nothing transforms.
    /// `None` for a derived stream, which has no recorded data, and for a key
    /// the system does not hold.
    #[must_use]
    pub fn raw(&self, key: &str) -> Option<&Sequence> {
        self.raw
            .get(key)
            .or_else(|| self.members.get(key).filter(|_| !self.is_derived(key)))
    }

    /// Whether the system holds a stream or a derived stream under `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.members.contains(key)
    }

    /// The number of streams and derived streams.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the system holds no stream and no derived stream.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
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
        let key = Key::from(key);
        self.members.is_derived(key.as_str()) && self.pipeline.producer(&key).is_some()
    }

    /// The processor that produces the derived stream under `key`; for a
    /// transformed stream, the last of its transforms, the one that gives its
    /// last version.
    #[must_use]
    pub fn producer(&self, key: &str) -> Option<&Processor> {
        if self.raw.contains(key) {
            return self.transforms(key).pop();
        }
        if !self.members.is_derived(key) {
            return None;
        }
        self.pipeline.producer(&Key::from(key))
    }

    /// The processors that transform the stream under `key`, in the order they
    /// apply: each reads the version the one before it gives. Empty for a
    /// stream nothing transforms, and for a derived stream.
    #[must_use]
    pub fn transforms(&self, key: &str) -> Vec<&Processor> {
        let processors = self.pipeline.processors();
        self.pipeline
            .chain(&Key::from(key))
            .into_iter()
            .map(|index| &processors[index])
            .collect()
    }

    /// Whether the stream under `key` is rewritten by at least one transform.
    #[must_use]
    pub fn is_transformed(&self, key: &str) -> bool {
        self.raw.contains(key)
    }

    /// The entity the derived stream under `key` is about, for a member of a
    /// per-entity output.
    #[must_use]
    pub fn entity(&self, key: &str) -> Option<&crate::values::Value> {
        self.members.derived_entity(key)
    }

    /// The streams and derived streams together, as a store.
    pub(crate) const fn members(&self) -> &Store {
        &self.members
    }

    /// The processors, in the order they are evaluated in.
    #[must_use]
    pub fn processors(&self) -> &[Processor] {
        self.pipeline.processors()
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

    /// The schema of the stream under `key`, if it has one. A transformed
    /// stream has the schema of its recorded data.
    #[must_use]
    pub fn schema(&self, key: &str) -> Option<&Schema> {
        self.raw.schema(key).or_else(|| self.members.schema(key))
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
        let bounds = (range.start_bound().cloned(), range.end_bound().cloned());
        StreamSystem {
            members: self.members.slice(bounds),
            raw: self.raw.slice(bounds),
            pipeline: self.pipeline.clone(),
            mode: Batch,
        }
    }

    /// Refuse to write into `key` if it is derived or a processor's output.
    pub(super) fn writable(&self, key: &Key) -> Result<(), UpdateError> {
        if self.pipeline.producer(key).is_some() || self.is_derived(key.as_str()) {
            return Err(UpdateError::Derived { key: key.clone() });
        }
        Ok(())
    }

    /// The output key of a processor that reads `key`, if any does.
    pub(super) fn reader_of(&self, key: &Key) -> Option<&Key> {
        self.pipeline.reader_of(key)
    }
}

/// Put `sequence` under `key` in `members`, as a version of a stream: the
/// member keeps its metadata and is not derived.
fn put_version(members: &mut Store, key: &Key, sequence: Sequence) {
    let metadata = members.member_metadata(key.as_str()).cloned();
    members.insert(key.clone(), sequence);
    if let (Some(metadata), Some(kept)) = (metadata, members.member_metadata_mut(key.as_str())) {
        *kept = metadata;
    }
}

/// The name of a sequence's shape, for [`ProcessError::TransformShape`].
const fn shape(sequence: &Sequence) -> &'static str {
    match sequence {
        Sequence::TimePoint(_) => "points",
        Sequence::TimeInterval(_) => "timeline",
    }
}

/// Move the stream under `key` out of `members` into `raw`, where its recorded
/// data stays while transforms rewrite the member.
pub(super) fn shelve(members: &Store, raw: &mut Store, key: &Key) {
    let sequence = members
        .get(key.as_str())
        .expect("a stream is held before it is transformed")
        .clone();
    raw.insert(key.clone(), sequence);
    if let Some(schema) = members.schema(key.as_str()) {
        raw.set_schema(key.as_str(), schema.clone())
            .expect("the data already fits its schema");
    }
}

/// Take the stream under `key` back out of `raw` into `members`, once nothing
/// transforms it.
pub(super) fn unshelve(members: &mut Store, raw: &mut Store, key: &Key) {
    let schema = raw.schema(key.as_str()).cloned();
    let sequence = raw
        .remove(key.as_str())
        .expect("a transformed stream has recorded data");
    put_version(members, key, sequence);
    if let Some(schema) = schema {
        members
            .set_schema(key.as_str(), schema)
            .expect("the data already fits its schema");
    }
}

/// Evaluate again the processors of `pipeline` at `indices`, in list order.
/// Each processor's members in `members` replace the ones it wrote before,
/// keeping their metadata. A transform reads the version before it: the first
/// of a chain starts from the recorded data in `raw`, and `indices` holds a
/// whole chain or none of it. Returns the derived and transformed streams whose
/// content changed.
///
/// With `updated`, the keys of the streams that were just changed, a processor
/// that is not a transform runs only if one of its inputs is among them or is a
/// stream a processor before it changed. The members of a per-entity output are
/// matched one by one.
///
/// On an error, `members` may be half updated: callers work on a copy.
pub(super) fn evaluate(
    pipeline: &Pipeline,
    indices: &[usize],
    members: &mut Store,
    raw: &Store,
    updated: Option<&BTreeSet<Key>>,
) -> Result<Report, UpdateError> {
    let mut report = Report::default();
    let mut previous: BTreeMap<Key, Option<Sequence>> = BTreeMap::new();
    let chains = pipeline.chains();

    for &index in indices {
        let processor = &pipeline.processors()[index];
        let output = processor.output();
        if let Some(updated) = updated {
            let moved = |key: &Key| updated.contains(key) || report.changed.contains(key);
            if !processor.is_transform() && !processor.inputs().into_iter().any(moved) {
                continue;
            }
        }
        report.evaluated.insert(output.clone());
        let failed = |error| UpdateError::Process {
            processor: output.clone(),
            error,
        };

        if processor.is_transform() {
            if chains[output].first() == Some(&index) {
                previous.insert(output.clone(), members.get(output.as_str()).cloned());
                let recorded = raw
                    .get(output.as_str())
                    .expect("a transformed stream has recorded data")
                    .clone();
                put_version(members, output, recorded);
            }

            let mut fresh = processor.run(members).map_err(failed)?;
            let version = fresh
                .remove(output.as_str())
                .expect("a transform produces its own stream");
            let kept = members
                .get(output.as_str())
                .expect("a transformed stream is held");
            if shape(kept) != shape(&version) {
                return Err(failed(ProcessError::TransformShape {
                    key: output.clone(),
                    expected: shape(kept),
                    found: shape(&version),
                }));
            }
            put_version(members, output, version);

            if chains[output].last() == Some(&index)
                && previous.get(output).and_then(Option::as_ref) != members.get(output.as_str())
            {
                report.changed.insert(output.clone());
            }
            continue;
        }

        let mut fresh = processor.run(members).map_err(failed)?;

        let before: BTreeSet<Key> = members
            .keys()
            .filter(|key| writes(processor, key) && members.is_derived(key.as_str()))
            .cloned()
            .collect();
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
            let entity = fresh.derived_entity(key.as_str()).cloned();
            let sequence = fresh
                .remove(key.as_str())
                .expect("every key listed is held");
            if members.get(key.as_str()) != Some(&sequence) {
                report.changed.insert(key.clone());
            }

            let metadata = members.member_metadata(key.as_str()).cloned();
            members.insert_derived(key.clone(), sequence, entity);
            if let (Some(metadata), Some(kept)) =
                (metadata, members.member_metadata_mut(key.as_str()))
            {
                *kept = metadata;
            }
        }
    }

    Ok(report)
}
