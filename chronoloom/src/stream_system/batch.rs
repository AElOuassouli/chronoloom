//! The batch mode of a stream system: a dataset held whole, whose derived
//! streams are evaluated again whenever what they read changes.

use std::collections::{BTreeMap, BTreeSet};

use crate::sequence_system::{Key, RecordError, Sequence, SequenceSystem};
use crate::sequences::Extent;
use crate::values::Schema;

use super::shared::evaluate;
use crate::processor::Processor;

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
            processors: Vec::new(),
            produced: BTreeMap::new(),
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
    /// [`UpdateError::Derived`] if `key` is a derived stream or the output of
    /// a processor, and [`UpdateError::Process`] if a processor that reads it
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
    /// stream or another processor's output, and [`UpdateError::Process`] if
    /// it fails: it reads a key the system does not hold (its own output
    /// included), or a member of another shape or value type than its formula
    /// or its declarations say. Nothing changes in any case.
    pub fn add_processor(&mut self, processor: Processor) -> Result<Report, UpdateError> {
        let output = processor.output();
        if self.members.contains(output.as_str()) || self.produced.contains_key(output) {
            return Err(UpdateError::Taken {
                key: output.clone(),
            });
        }

        // Nothing reads the new output yet, so it is the only one to evaluate.
        let mut members = self.members.clone();
        let mut produced = self.produced.clone();
        let report = evaluate(
            core::slice::from_ref(&processor),
            &mut members,
            &mut produced,
            None,
        )?;

        self.members = members;
        self.produced = produced;
        self.processors.push(processor);
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
        let index = self
            .processors
            .iter()
            .position(|processor| processor.output().as_str() == output)
            .ok_or_else(|| UpdateError::UnknownProcessor { key: output.into() })?;

        let written = self.produced.get(output).cloned().unwrap_or_default();
        for key in written.iter().chain([self.processors[index].output()]) {
            let reader = self
                .processors
                .iter()
                .find(|other| other.output().as_str() != output && other.inputs().contains(key));
            if let Some(reader) = reader {
                return Err(UpdateError::Read {
                    key: key.clone(),
                    by: reader.output().clone(),
                });
            }
        }

        for key in &written {
            self.members.remove(key.as_str());
        }
        self.produced.remove(output);
        Ok(self.processors.remove(index))
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
        if self.is_derived(key.as_str()) || self.produced.contains_key(key) {
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
        let mut produced = self.produced.clone();
        edit(&mut members)?;
        let report = evaluate(&self.processors, &mut members, &mut produced, changed)?;

        self.members = members;
        self.produced = produced;
        Ok(report)
    }
}
