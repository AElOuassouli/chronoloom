//! The processors of a stream system, in the one order they are evaluated in,
//! and who reads what.

use std::collections::{BTreeMap, BTreeSet};

use crate::processing::{is_under, ProcessError, Recipe};
use crate::processor::Processor;
use crate::store::Key;

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
pub(crate) fn writes(processor: &Processor, key: &Key) -> bool {
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

    /// The processor that derives the member under `key`, if one does: a
    /// transform rewrites a stream and derives nothing.
    pub(crate) fn producer(&self, key: &Key) -> Option<&Processor> {
        self.processors
            .iter()
            .find(|processor| writes(processor, key) && !processor.is_transform())
    }

    /// The indices of the transforms of the stream under `key`, in chain order.
    pub(crate) fn chain(&self, key: &Key) -> Vec<usize> {
        self.processors
            .iter()
            .enumerate()
            .filter(|(_, processor)| processor.is_transform() && processor.output() == key)
            .map(|(index, _)| index)
            .collect()
    }

    /// Every chain of transforms, by the key of the stream they rewrite, found
    /// in one pass.
    pub(crate) fn chains(&self) -> BTreeMap<Key, Vec<usize>> {
        let mut chains: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
        for (index, processor) in self.processors.iter().enumerate() {
            if processor.is_transform() {
                chains
                    .entry(processor.output().clone())
                    .or_default()
                    .push(index);
            }
        }
        chains
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
    /// [`UpdateError::Taken`] if another processor has its output (a
    /// transform shares it with the transforms of the same stream only), or
    /// one output lies under the other's per-entity output,
    /// [`UpdateError::Read`] if it transforms a stream another processor
    /// already reads, which would then read a version that is not the last, and
    /// [`ProcessError::Cycle`] if a per-entity output reads what it writes.
    pub(crate) fn push(&mut self, processor: Processor) -> Result<(), UpdateError> {
        let transform = processor.is_transform();
        let taken = || UpdateError::Taken {
            key: processor.output().clone(),
        };
        let shares = |other: &Processor| {
            transform && other.is_transform() && other.output() == processor.output()
        };
        if self
            .processors
            .iter()
            .any(|other| other.output() == processor.output() && !shares(other))
        {
            return Err(taken());
        }
        let nested = self.processors.iter().any(|other| {
            !shares(other)
                && (writes(&processor, other.output()) || writes(other, processor.output()))
        });
        if nested {
            return Err(taken());
        }
        if transform {
            let key = processor.output();
            let reader = self.readers.get(key).and_then(|readers| {
                readers
                    .iter()
                    .find(|&&reader| !shares(&self.processors[reader]))
            });
            if let Some(&reader) = reader {
                return Err(UpdateError::Read {
                    key: key.clone(),
                    by: self.processors[reader].output().clone(),
                });
            }
        } else if processor
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
            .rposition(|processor| processor.output().as_str() == output)
            .ok_or_else(|| UpdateError::UnknownProcessor { key: output.into() })?;

        let processor = &self.processors[index];
        for (key, readers) in &self.readers {
            if !writes(processor, key) {
                continue;
            }
            // An earlier transform of the same stream reads what comes before
            // this one, not what it writes.
            let reads_it = |&&reader: &&usize| {
                let other = &self.processors[reader];
                reader != index
                    && !(processor.is_transform() && other.is_transform() && reader < index)
            };
            if let Some(&reader) = readers.iter().find(reads_it) {
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

        let chains = self.chains();

        // A reader comes after what it reads, so one pass in order reaches
        // them all; but a transform that changed brings its whole chain, from
        // the first, which an earlier index may be, so the pass is repeated
        // until nothing is added.
        loop {
            let known = affected.len();
            for index in 0..self.processors.len() {
                if !affected.contains(&index) {
                    continue;
                }
                let processor = &self.processors[index];
                if processor.is_transform() {
                    affected.extend(&chains[processor.output()]);
                }
                for (key, readers) in &self.readers {
                    if writes(processor, key) {
                        affected.extend(readers);
                    }
                }
            }
            if affected.len() == known {
                break;
            }
        }
        affected.into_iter().collect()
    }
}
