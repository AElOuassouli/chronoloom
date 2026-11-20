//! The batch mode of a stream system: a dataset held whole, whose derived
//! streams are evaluated again whenever what they read changes.

use std::collections::BTreeSet;

use crate::sequence_system::{Key, RecordError, Sequence, SequenceSystem};
use crate::sequences::Extent;
use crate::values::Schema;

use crate::processing::Recipe;
use crate::processor::Processor;

use super::pipeline::{writes, Pipeline};
use super::shared::evaluate;
use super::{Batch, Report, StreamSystem, Update, UpdateError};

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
            pipeline: Pipeline::default(),
            mode: Batch,
        }
    }

    /// Record `update` into the streams, then evaluate again the processors
    /// that read what it changed, directly or through another processor.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] if it records into a derived stream,
    /// [`UpdateError::Record`] if recording fails (an unknown key, another
    /// shape or value type, a schema mismatch), and [`UpdateError::Process`]
    /// if a processor it affects fails. Nothing changes in any case.
    pub fn update(&mut self, update: Update) -> Result<Report, UpdateError> {
        let keys: BTreeSet<Key> = update.chunks.iter().map(|(key, _)| key.clone()).collect();
        if let Some(key) = keys.iter().find(|key| self.is_derived(key.as_str())) {
            return Err(UpdateError::Derived { key: key.clone() });
        }

        self.change(Some(keys), |members| {
            members.record_update(update)?;
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
        self.change(Some(BTreeSet::from([key.clone()])), |members| {
            members.insert(key, sequence);
            Ok(())
        })
    }

    /// Take the stream under `key` out of the system.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] if `key` is a derived stream, and
    /// [`UpdateError::Read`] if a processor reads it. Nothing changes in
    /// either case.
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
    /// # Errors
    ///
    /// [`UpdateError::Taken`] if its output key is already a stream, a derived
    /// stream or another processor's output, if it lies under the per-entity
    /// output of another processor, or if it is a per-entity output over keys
    /// the system already holds, and [`UpdateError::Process`] if
    /// it fails: it reads a key the system does not hold (its own output
    /// included), or a member of another shape or value type than its formula
    /// or its declarations say. Nothing changes in any case.
    pub fn add_processor(&mut self, processor: Processor) -> Result<Report, UpdateError> {
        let output = processor.output();
        let under = matches!(processor.recipe(), Recipe::PerEntity { .. })
            && self.members.keys().any(|key| writes(&processor, key));
        if self.members.contains(output.as_str()) || under {
            return Err(UpdateError::Taken {
                key: output.clone(),
            });
        }

        // Nothing reads the new output yet, so it is the only one to evaluate.
        let mut pipeline = self.pipeline.clone();
        pipeline.push(processor)?;
        let mut members = self.members.clone();
        let last = pipeline.processors().len() - 1;
        let report = evaluate(&pipeline, &[last], &mut members)?;

        self.members = members;
        self.pipeline = pipeline;
        Ok(report)
    }

    /// Take the processor producing `output` out of the list, and its derived
    /// streams with it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::UnknownProcessor`] if no processor produces `output`,
    /// and [`UpdateError::Read`] if another processor reads what it produces.
    /// Nothing changes in either case.
    pub fn remove_processor(&mut self, output: &str) -> Result<Processor, UpdateError> {
        let processor = self.pipeline.remove(output)?;
        let written: Vec<Key> = self
            .members
            .keys()
            .filter(|key| writes(&processor, key) && self.members.lineage(key.as_str()).is_some())
            .cloned()
            .collect();
        for key in &written {
            self.members.remove(key.as_str());
        }
        Ok(processor)
    }

    /// Replace the attribute of the stream under `key`, keeping its data.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] for a derived stream, whose label is its
    /// formula, and [`UpdateError::Record`] for an unknown key.
    pub fn relabel(&mut self, key: &str, attribute: impl Into<String>) -> Result<(), UpdateError> {
        self.writable(&Key::from(key))?;
        Ok(self.members.relabel(key, attribute)?)
    }

    /// Give the stream under `key` a schema, which its records and every later
    /// update must fit.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] for a derived stream, and
    /// [`UpdateError::Record`] as [`SequenceSystem::set_schema`] says.
    pub fn set_schema(&mut self, key: &str, schema: Schema) -> Result<(), UpdateError> {
        self.writable(&Key::from(key))?;
        Ok(self.members.set_schema(key, schema)?)
    }

    /// Narrow every stream to `extent`, then evaluate every processor again.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Process`] if a processor fails. Nothing changes then.
    pub fn observe(&mut self, extent: &Extent) -> Result<Report, UpdateError> {
        self.change(None, |members| {
            members.observe(extent);
            Ok(())
        })
    }

    /// Refuse to write into `key` if it is derived or a processor's output.
    fn writable(&self, key: &Key) -> Result<(), UpdateError> {
        if self.pipeline.producer(key).is_some() || self.is_derived(key.as_str()) {
            return Err(UpdateError::Derived { key: key.clone() });
        }
        Ok(())
    }

    /// Apply `edit` to a copy of the members, evaluate the processors it
    /// affects (`None`: all of them), and keep the result only if both succeed.
    fn change(
        &mut self,
        changed: Option<BTreeSet<Key>>,
        edit: impl FnOnce(&mut SequenceSystem) -> Result<(), RecordError>,
    ) -> Result<Report, UpdateError> {
        let mut members = self.members.clone();
        edit(&mut members)?;
        let indices = changed.map_or_else(
            || (0..self.pipeline.processors().len()).collect(),
            |changed| self.pipeline.affected(&changed),
        );
        let report = evaluate(&self.pipeline, &indices, &mut members)?;

        self.members = members;
        Ok(report)
    }
}
