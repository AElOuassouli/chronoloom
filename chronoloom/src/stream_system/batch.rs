//! The batch mode of a stream system: a dataset held whole, whose derived
//! streams are evaluated again whenever what they read changes.

use std::collections::BTreeSet;

use crate::primitives::Timestamp;
use crate::sequence_system::{Key, Metadata, RecordError, Sequence, SequenceSystem};
use crate::sequences::Extent;
use crate::values::Schema;

use crate::processing::{ProcessError, Recipe};
use crate::processor::Processor;

use super::pipeline::{writes, Pipeline};
use super::shared::{evaluate, shelve, unshelve};
use super::{Batch, Live, Report, StreamSystem, Update, UpdateError};

impl Default for StreamSystem<Batch> {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSystem<Batch> {
    /// An empty system in batch mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            members: SequenceSystem::new(),
            raw: SequenceSystem::new(),
            pipeline: Pipeline::default(),
            mode: Batch,
        }
    }

    /// Record `update` into the streams, then evaluate again the processors
    /// that read what it changed, directly or through another processor.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Watermark`] if it carries a watermark, which a batch
    /// system has no frontier for, [`UpdateError::Derived`] if it records into
    /// a derived stream, [`UpdateError::Record`] if recording fails (an
    /// unknown key, another shape or value type, a schema mismatch), and [`UpdateError::Process`]
    /// if a processor it affects fails. Nothing changes in any case.
    pub fn update(&mut self, update: Update) -> Result<Report, UpdateError> {
        if !update.watermarks.is_empty() {
            return Err(UpdateError::Watermark);
        }
        let keys: BTreeSet<Key> = update.chunks.iter().map(|(key, _)| key.clone()).collect();
        if let Some(key) = keys.iter().find(|key| self.is_derived(key.as_str())) {
            return Err(UpdateError::Derived { key: key.clone() });
        }

        // A transformed stream is recorded into as the recorded data, and its
        // versions follow.
        let (recorded, plain) = update.split(|key| self.raw.contains(key.as_str()));
        self.change(Some(keys), |members, raw| {
            members.record_update(plain)?;
            raw.record_update(recorded)?;
            Ok(())
        })
    }

    /// Add or replace the stream under `key`, then evaluate again the
    /// processors that read it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] if `key` is a derived stream, the output of a
    /// processor or a key under its per-entity output, and [`UpdateError::Process`] if a processor that reads it
    /// fails over the new stream (another shape or value type, say). Nothing
    /// changes in any case.
    pub fn insert_stream(
        &mut self,
        key: impl Into<Key>,
        sequence: impl Into<Sequence>,
    ) -> Result<Report, UpdateError> {
        let key = key.into();
        self.writable(&key)?;

        let sequence = sequence.into();
        self.change(Some(BTreeSet::from([key.clone()])), |members, raw| {
            if raw.contains(key.as_str()) {
                // As for a plain stream, a replacement starts without metadata.
                if let Some(metadata) = members.member_metadata_mut(key.as_str()) {
                    *metadata = Metadata::new();
                }
                raw.insert(key, sequence);
            } else {
                members.insert(key, sequence);
            }
            Ok(())
        })
    }

    /// Take the stream under `key` out of the system.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] if `key` is a derived stream, and
    /// [`UpdateError::Read`] if a processor reads it, a transform of the
    /// stream included. Nothing changes in either case.
    pub fn remove_stream(&mut self, key: &str) -> Result<Option<Sequence>, UpdateError> {
        let key = Key::from(key);
        self.writable(&key)?;
        if let Some(by) = self.reader_of(&key) {
            return Err(UpdateError::Read {
                key,
                by: by.clone(),
            });
        }

        Ok(self.members.remove(key.as_str()))
    }

    /// Add a processor at the end of the list and evaluate its output.
    ///
    /// A processor whose output key is a stream it reads is a transform of
    /// that stream: it rewrites it, and the stream keeps its recorded data
    /// apart (see [`raw`](StreamSystem::raw)). Transforms of one stream chain
    /// in the order they are added, each reading the version before it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Taken`] if its output key is already a stream it does
    /// not read, a derived stream or another processor's output, if it lies
    /// under the per-entity output of another processor, or if it is a
    /// per-entity output over keys the system already holds,
    /// [`UpdateError::Read`] if it transforms a stream another processor
    /// reads (that one would not read the last version), and
    /// [`UpdateError::Process`] if it fails: it reads a key the system does
    /// not hold (its own output included, unless it is a stream to transform),
    /// a member of another shape or value type than its formula or its
    /// declarations say, or, as a transform, it produces another shape than the
    /// stream has ([`ProcessError::TransformShape`]). Nothing changes in any
    /// case.
    ///
    /// [`ProcessError::TransformShape`]: crate::processing::ProcessError::TransformShape
    pub fn add_processor(&mut self, processor: Processor) -> Result<Report, UpdateError> {
        let output = processor.output();
        let transform = processor.is_transform();
        if transform {
            if !self.members.contains(output.as_str()) {
                return Err(UpdateError::Process {
                    processor: output.clone(),
                    error: ProcessError::UnknownKey {
                        key: output.clone(),
                    },
                });
            }
            if self.is_derived(output.as_str()) {
                return Err(UpdateError::Taken {
                    key: output.clone(),
                });
            }
        } else {
            let under = matches!(processor.recipe(), Recipe::PerEntity { .. })
                && self.members.keys().any(|key| writes(&processor, key));
            if self.members.contains(output.as_str()) || under {
                return Err(UpdateError::Taken {
                    key: output.clone(),
                });
            }
        }

        // Nothing reads the new output yet, so it is the only one to evaluate,
        // with the rest of its chain if it is a transform.
        let mut pipeline = self.pipeline.clone();
        pipeline.push(processor)?;
        let mut members = self.members.clone();
        let mut raw = self.raw.clone();
        let last = pipeline.processors().len() - 1;
        let indices = if transform {
            let key = pipeline.processors()[last].output().clone();
            if !raw.contains(key.as_str()) {
                shelve(&members, &mut raw, &key);
            }
            pipeline.chain(&key)
        } else {
            vec![last]
        };
        let report = evaluate(&pipeline, &indices, &mut members, &raw)?;

        self.members = members;
        self.raw = raw;
        self.pipeline = pipeline;
        Ok(report)
    }

    /// Take the processor producing `output` out of the list, and its derived
    /// streams with it. For a transformed stream that is its last transform,
    /// and the stream goes back to the version before it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::UnknownProcessor`] if no processor produces `output`,
    /// and [`UpdateError::Read`] if another processor reads what it produces:
    /// for a transform, any processor that is not an earlier transform of the
    /// stream. Nothing changes in either case.
    pub fn remove_processor(&mut self, output: &str) -> Result<Processor, UpdateError> {
        let mut pipeline = self.pipeline.clone();
        let processor = pipeline.remove(output)?;
        let mut members = self.members.clone();
        let mut raw = self.raw.clone();

        if processor.is_transform() {
            let key = processor.output();
            let chain = pipeline.chain(key);
            if chain.is_empty() {
                unshelve(&mut members, &mut raw, key);
            } else {
                evaluate(&pipeline, &chain, &mut members, &raw)?;
            }
        } else {
            let written: Vec<Key> = members
                .keys()
                .filter(|key| writes(&processor, key) && members.lineage(key.as_str()).is_some())
                .cloned()
                .collect();
            for key in &written {
                members.remove(key.as_str());
            }
        }

        self.members = members;
        self.raw = raw;
        self.pipeline = pipeline;
        Ok(processor)
    }

    /// Replace the attribute of the stream under `key`, keeping its data.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] for a derived stream, whose label is its
    /// formula, and [`UpdateError::Record`] for an unknown key.
    pub fn relabel(&mut self, key: &str, attribute: impl Into<String>) -> Result<(), UpdateError> {
        let key = Key::from(key);
        self.writable(&key)?;
        if self.raw.contains(key.as_str()) {
            let attribute = attribute.into();
            self.change(Some(BTreeSet::from([key.clone()])), |_, raw| {
                raw.relabel(key.as_str(), attribute)
            })?;
            return Ok(());
        }
        Ok(self.members.relabel(key.as_str(), attribute)?)
    }

    /// Give the stream under `key` a schema, which its records and every later
    /// update must fit.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] for a derived stream, and
    /// [`UpdateError::Record`] as [`SequenceSystem::set_schema`] says.
    pub fn set_schema(&mut self, key: &str, schema: Schema) -> Result<(), UpdateError> {
        let key = Key::from(key);
        self.writable(&key)?;
        if self.raw.contains(key.as_str()) {
            self.change(Some(BTreeSet::from([key.clone()])), |_, raw| {
                raw.set_schema(key.as_str(), schema)
            })?;
            return Ok(());
        }
        Ok(self.members.set_schema(key.as_str(), schema)?)
    }

    /// Narrow every stream to `extent`, then evaluate every processor again.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Process`] if a processor fails. Nothing changes then.
    pub fn observe(&mut self, extent: &Extent) -> Result<Report, UpdateError> {
        self.change(None, |members, raw| {
            members.observe(extent);
            raw.observe(extent);
            Ok(())
        })
    }

    /// This system in live mode, starting at `from`: every stream is cut to
    /// what lies before `from`, which is final, and each processor starts by
    /// stepping over it, so the derived streams are what the processors give
    /// over the data so far. Later updates carry events at or after `from`, and
    /// the result equals that of a live system which was given the same events
    /// from the start.
    ///
    /// Beyond `from` each stream is observed everywhere, as a stream added to a
    /// live system is.
    ///
    /// The derived streams are computed again, not cut: a presence that began
    /// before `from` and is still open reaches `from` only. Metadata, labels
    /// and schemas are kept, those of a derived stream too.
    ///
    /// # Errors
    ///
    /// [`UpdateError::LiveTransform`] if a processor transforms a stream,
    /// which a live system does not run yet, and [`UpdateError::Process`] if a
    /// processor fails over the cut streams.
    pub fn into_live(self, from: Timestamp) -> Result<StreamSystem<Live>, UpdateError> {
        let mut live = StreamSystem::live(from);
        live.members = self.members.slice(..from);
        for key in self.derived() {
            live.members.remove(key.as_str());
        }
        let streams: Vec<Key> = live.members.keys().cloned().collect();
        for key in streams {
            live.members.observe_from(key.as_str(), from);
            live.mode.frontiers.insert(key, from);
        }

        for processor in self.pipeline.processors() {
            live.add_processor(processor.clone())?;
        }
        for key in self.derived() {
            let metadata = self.member_metadata(key.as_str());
            if let (Some(metadata), Some(kept)) = (metadata, live.member_metadata_mut(key.as_str()))
            {
                *kept = metadata.clone();
            }
        }
        Ok(live)
    }

    /// Apply `edit` to a copy of the members, evaluate the processors it
    /// affects (`None`: all of them), and keep the result only if both succeed.
    fn change(
        &mut self,
        changed: Option<BTreeSet<Key>>,
        edit: impl FnOnce(&mut SequenceSystem, &mut SequenceSystem) -> Result<(), RecordError>,
    ) -> Result<Report, UpdateError> {
        let mut members = self.members.clone();
        let mut raw = self.raw.clone();
        edit(&mut members, &mut raw)?;
        let indices = changed.map_or_else(
            || (0..self.pipeline.processors().len()).collect(),
            |changed| self.pipeline.affected(&changed),
        );
        let report = evaluate(&self.pipeline, &indices, &mut members, &raw)?;

        self.members = members;
        self.raw = raw;
        Ok(report)
    }
}
