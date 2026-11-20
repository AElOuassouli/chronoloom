//! The processors of a stream system, in the one order they are evaluated in,
//! and who reads what.

use std::collections::{BTreeMap, BTreeSet};

use crate::processing::{is_under, ProcessError, Recipe};
use crate::processor::Processor;
use crate::sequence_system::Key;

use super::UpdateError;

/// The processors of a system, each after what it reads, with an index from
/// every key read to the processors that read it.
///
/// A processor reading a key under a per-entity output reads that output.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Pipeline {
    processors: Vec<Processor>,
    readers: BTreeMap<Key, BTreeSet<usize>>,
}

/// Whether `processor` writes the member under `key`: its output for most
/// recipes, a key under it for a per-entity one.
pub(super) fn writes(processor: &Processor, key: &Key) -> bool {
    match processor.recipe() {
        Recipe::PerEntity { .. } => is_under(key, processor.output()),
        _ => processor.output() == key,
    }
}

impl Pipeline {
    /// The processors, in evaluation order.
    pub(crate) fn processors(&self) -> &[Processor] {
        &self.processors
    }

    /// Whether a processor has `key` as its output.
    pub(crate) fn has_output(&self, key: &Key) -> bool {
        self.processors
            .iter()
            .any(|processor| processor.output() == key)
    }

    /// The processor that writes the member under `key`, if one does.
    pub(crate) fn producer(&self, key: &Key) -> Option<&Processor> {
        self.processors
            .iter()
            .find(|processor| writes(processor, key))
    }

    /// The first processor that reads the member under `key`, by its output.
    pub(crate) fn reader_of(&self, key: &Key) -> Option<&Key> {
        let index = *self.readers.get(key)?.first()?;
        Some(self.processors[index].output())
    }

    /// Add `processor` at the end of the list.
    ///
    /// It does not know the streams, so a read of a key nothing holds is not
    /// refused here: the evaluation in `add_processor` refuses it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Taken`] if another processor has its output, or one
    /// output lies under the other's per-entity output, and
    /// [`ProcessError::Cycle`] if it reads what it writes itself.
    pub(crate) fn push(&mut self, processor: Processor) -> Result<(), UpdateError> {
        if self.has_output(processor.output()) {
            return Err(UpdateError::Taken {
                key: processor.output().clone(),
            });
        }
        let nested = self
            .processors
            .iter()
            .any(|other| writes(&processor, other.output()) || writes(other, processor.output()));
        if nested {
            return Err(UpdateError::Taken {
                key: processor.output().clone(),
            });
        }
        if processor
            .inputs()
            .into_iter()
            .any(|key| writes(&processor, key))
        {
            return Err(UpdateError::Process {
                processor: processor.output().clone(),
                error: ProcessError::Cycle {
                    keys: vec![processor.output().clone()],
                },
            });
        }

        let index = self.processors.len();
        for key in processor.inputs() {
            self.readers.entry(key.clone()).or_default().insert(index);
        }
        self.processors.push(processor);
        Ok(())
    }

    /// Take the processor producing `output` out of the list.
    ///
    /// # Errors
    ///
    /// [`UpdateError::UnknownProcessor`] if none does, and
    /// [`UpdateError::Read`] while another processor reads what it writes.
    pub(crate) fn remove(&mut self, output: &str) -> Result<Processor, UpdateError> {
        let index = self
            .processors
            .iter()
            .position(|processor| processor.output().as_str() == output)
            .ok_or_else(|| UpdateError::UnknownProcessor { key: output.into() })?;

        let processor = &self.processors[index];
        for (key, readers) in &self.readers {
            if !writes(processor, key) {
                continue;
            }
            if let Some(&reader) = readers.iter().find(|&&reader| reader != index) {
                return Err(UpdateError::Read {
                    key: key.clone(),
                    by: self.processors[reader].output().clone(),
                });
            }
        }

        let removed = self.processors.remove(index);
        self.readers.clear();
        for (index, processor) in self.processors.iter().enumerate() {
            for key in processor.inputs() {
                self.readers.entry(key.clone()).or_default().insert(index);
            }
        }
        Ok(removed)
    }

    /// The processors, by index in list order, that read a key of `changed`,
    /// directly or through what another affected processor writes.
    pub(crate) fn affected(&self, changed: &BTreeSet<Key>) -> Vec<usize> {
        let mut affected: BTreeSet<usize> = changed
            .iter()
            .filter_map(|key| self.readers.get(key))
            .flatten()
            .copied()
            .collect();

        // A reader comes after what it reads, so one pass in order is enough.
        for index in 0..self.processors.len() {
            if !affected.contains(&index) {
                continue;
            }
            for (key, readers) in &self.readers {
                if writes(&self.processors[index], key) {
                    affected.extend(readers);
                }
            }
        }
        affected.into_iter().collect()
    }
}
