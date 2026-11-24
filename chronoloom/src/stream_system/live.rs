//! The live mode of a stream system: data arrives in time order, each stream
//! has a frontier before which everything is final, and the processors run
//! incrementally over what became final.

use std::collections::{BTreeMap, BTreeSet};

use crate::operator::{compile_recipe, Chunk, Inputs};
use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{check_declared, member_key, render, ProcessError, Recipe};
use crate::processor::Processor;
use crate::sequence_system::{Key, RecordError, Sequence, SequenceSystem};

use super::mode::LiveNode;
use super::pipeline::{writes, Pipeline};
use super::update::Watermark;
use super::{Live, LiveReport, StreamSystem, Update, UpdateError};

impl StreamSystem<Live> {
    /// An empty system in live mode, whose streams are final before `from`:
    /// the frontier of every stream starts there, and an event before it is
    /// late.
    #[must_use]
    pub fn live(from: Timestamp) -> Self {
        Self {
            members: SequenceSystem::new(),
            raw: SequenceSystem::new(),
            pipeline: Pipeline::default(),
            mode: Live {
                from,
                frontiers: BTreeMap::new(),
                nodes: Vec::new(),
            },
        }
    }

    /// How far the stream or derived stream under `key` is final: everything
    /// before the instant is. `None` for a key the system does not hold.
    ///
    /// The frontier of a stream is moved by watermarks. That of a derived
    /// stream follows from those of what it reads: the earliest of them, less
    /// what its formula looks ahead.
    #[must_use]
    pub fn frontier(&self, key: &str) -> Option<Timestamp> {
        let key = Key::from(key);
        if !self.members.contains(key.as_str()) {
            return None;
        }
        frontier_of(&self.mode, &self.pipeline, &key)
    }

    /// Add the stream under `key`, which starts empty of finality: its
    /// frontier is where the system started, so the events `sequence` holds
    /// must not be before it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Derived`] if `key` is a derived stream or lies under a
    /// processor's output, [`UpdateError::Taken`] if the system already holds
    /// it (a live stream is never replaced: what it gave the processors is
    /// final), and [`UpdateError::Late`] if `sequence` starts before the
    /// system. Nothing changes in any case.
    pub fn insert_stream(
        &mut self,
        key: impl Into<Key>,
        sequence: impl Into<Sequence>,
    ) -> Result<(), UpdateError> {
        let key = key.into();
        self.writable(&key)?;
        if self.members.contains(key.as_str()) {
            return Err(UpdateError::Taken { key });
        }

        let sequence = sequence.into();
        if let Some(at) = earliest(&sequence) {
            if at < self.mode.from {
                return Err(UpdateError::Late {
                    key,
                    at,
                    frontier: self.mode.from,
                });
            }
        }

        self.members.insert(key.clone(), sequence);
        self.mode.frontiers.insert(key, self.mode.from);
        Ok(())
    }

    /// Add a processor at the end of the list and run it over what its inputs
    /// hold that is final already.
    ///
    /// The report gives what became final in its output.
    ///
    /// # Errors
    ///
    /// [`UpdateError::LiveTransform`] if it rewrites a stream it reads,
    /// [`UpdateError::Taken`] if its output key is already in use or lies
    /// under the per-entity output of another processor,
    /// [`UpdateError::Process`] if it reads a key the system does not hold or
    /// a member of another shape or value type than it expects. Nothing
    /// changes in any case.
    pub fn add_processor(&mut self, processor: Processor) -> Result<LiveReport, UpdateError> {
        let output = processor.output().clone();
        if processor.is_transform() {
            return Err(UpdateError::LiveTransform { key: output });
        }
        let under = matches!(processor.recipe(), Recipe::PerEntity { .. })
            && self.members.keys().any(|key| writes(&processor, key));
        if self.members.contains(output.as_str()) || under {
            return Err(UpdateError::Taken { key: output });
        }

        let failed = |error| UpdateError::Process {
            processor: output.clone(),
            error,
        };
        let mut next = self.clone();
        next.pipeline.push(processor.clone())?;
        check_declared(
            processor.declared_inputs(),
            |key| key == &output,
            &next.members,
        )
        .map_err(failed)?;
        if let Some(key) = processor
            .inputs()
            .into_iter()
            .find(|key| !next.members.contains(key.as_str()))
        {
            return Err(failed(ProcessError::UnknownKey { key: key.clone() }));
        }

        next.mode.nodes.push(LiveNode {
            recipe: compile_recipe(processor.recipe()).online(&output),
            read: BTreeMap::new(),
        });
        let mut chunks = BTreeMap::new();
        next.step(next.mode.nodes.len() - 1, &mut chunks)?;

        let report = next.report(chunks);
        *self = next;
        Ok(report)
    }

    /// Record `update` into the streams, move the frontiers it carries, and
    /// run the processors that read a stream whose frontier moved, directly or
    /// through another processor's output, over the range that became final.
    /// What they give is appended in place, so a step costs what is new.
    ///
    /// The events are recorded first, so an update may carry a point and the
    /// watermark that passes it.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Late`] if an event starts before the frontier its stream
    /// had before the update, [`UpdateError::Derived`] if it records into or
    /// advances a derived stream, [`UpdateError::Record`] if recording fails
    /// (an unknown key, another shape or value type, a schema mismatch) or a
    /// watermark names an unknown key, and [`UpdateError::Process`] if a
    /// processor fails. Nothing changes in any case.
    ///
    /// Known limit: all or nothing is kept by working on a copy of the system,
    /// so an update costs a copy of everything the system holds besides the
    /// evaluation.
    pub fn update(&mut self, update: Update) -> Result<LiveReport, UpdateError> {
        let mut next = self.clone();
        let report = next.apply(update)?;

        *self = next;
        Ok(report)
    }

    /// Move the frontier of the stream under `key` to `watermark`, as an
    /// update carrying only that watermark does.
    ///
    /// # Errors
    ///
    /// As [`update`](StreamSystem::update).
    pub fn advance(
        &mut self,
        key: impl Into<Key>,
        watermark: Timestamp,
    ) -> Result<LiveReport, UpdateError> {
        let mut update = Update::new();
        update.advance(key, watermark);
        self.update(update)
    }

    /// Move the frontier of every stream to `watermark`, as an update
    /// carrying only that watermark does.
    ///
    /// # Errors
    ///
    /// As [`update`](StreamSystem::update).
    pub fn advance_all(&mut self, watermark: Timestamp) -> Result<LiveReport, UpdateError> {
        let mut update = Update::new();
        update.advance_all(watermark);
        self.update(update)
    }

    /// [`update`](StreamSystem::update) on the system itself, which the caller
    /// works on a copy of.
    fn apply(&mut self, update: Update) -> Result<LiveReport, UpdateError> {
        let Update { chunks, watermarks } = update;

        for (key, pending) in &chunks {
            if self.is_derived(key.as_str()) {
                return Err(UpdateError::Derived { key: key.clone() });
            }
            let late = pending
                .earliest()
                .zip(self.mode.frontiers.get(key))
                .filter(|(at, frontier)| at < frontier);
            if let Some((at, &frontier)) = late {
                return Err(UpdateError::Late {
                    key: key.clone(),
                    at,
                    frontier,
                });
            }
        }
        self.members.record_update(Update {
            chunks,
            watermarks: Vec::new(),
        })?;

        let mut moved: BTreeSet<Key> = BTreeSet::new();
        for watermark in watermarks {
            match watermark {
                Watermark::Stream(key, watermark) => {
                    let Some(frontier) = self.mode.frontiers.get_mut(&key) else {
                        return Err(if self.members.contains(key.as_str()) {
                            UpdateError::Derived { key }
                        } else {
                            RecordError::Unknown {
                                key: key.as_str().to_owned(),
                            }
                            .into()
                        });
                    };
                    if watermark > *frontier {
                        *frontier = watermark;
                        moved.insert(key);
                    }
                }
                Watermark::All(watermark) => {
                    for (key, frontier) in &mut self.mode.frontiers {
                        if watermark > *frontier {
                            *frontier = watermark;
                            moved.insert(key.clone());
                        }
                    }
                }
            }
        }

        let mut chunks = BTreeMap::new();
        for index in self.pipeline.affected(&moved) {
            self.step(index, &mut chunks)?;
        }
        Ok(self.report(chunks))
    }

    /// Run the processor at `index` over the range that became final in what
    /// it reads, and append what it gives to its derived streams. The chunks
    /// that made progress are put in `chunks`, by key.
    fn step(&mut self, index: usize, chunks: &mut BTreeMap<Key, Chunk>) -> Result<(), UpdateError> {
        let processor = self.pipeline.processors()[index].clone();
        let output = processor.output();
        let failed = |error| UpdateError::Process {
            processor: output.clone(),
            error,
        };

        let mut inputs = Inputs::new();
        let mut reached = BTreeMap::new();
        let mut to = Timestamp::MIN;
        for key in processor.inputs() {
            let held = self.members.get(key.as_str());
            let frontier = frontier_of(&self.mode, &self.pipeline, key);
            let (Some(sequence), Some(frontier)) = (held, frontier) else {
                continue;
            };
            let from = self.mode.nodes[index]
                .read
                .get(key)
                .copied()
                .unwrap_or(Timestamp::MIN);
            inputs.insert(key.clone(), Chunk::of(sequence, from, frontier));
            reached.insert(key.clone(), frontier);
            to = to.max(frontier);
        }

        let node = &mut self.mode.nodes[index];
        let produced = node.recipe.step(&inputs, to).map_err(failed)?;
        node.read.extend(reached);

        for member in produced {
            let (key, entity) = match member.entity {
                None => (output.clone(), None),
                Some(entity) => (member_key(output, &render(&entity)), Some(entity)),
            };
            let chunk = member.chunk;

            if self.members.contains(key.as_str()) {
                if self.members.lineage(key.as_str()).is_none() {
                    return Err(UpdateError::Taken { key });
                }
                self.members.append_derived(key.as_str(), chunk.sequence());
            } else {
                let mut sequence = chunk.clone().into_sequence();
                if let (Recipe::Timeline(formula), Sequence::TimeInterval(timeline)) =
                    (processor.recipe(), &mut sequence)
                {
                    timeline.set_attribute(formula.to_string());
                }
                self.members.insert_derived(
                    key.clone(),
                    sequence,
                    processor.recipe().clone(),
                    entity,
                );
            }

            if !chunk.range().is_empty() {
                chunks.insert(key, chunk);
            }
        }
        Ok(())
    }

    /// The report of a step that gave `chunks`.
    fn report(&self, chunks: BTreeMap<Key, Chunk>) -> LiveReport {
        let frontiers = self
            .derived()
            .filter_map(|key| Some((key.clone(), frontier_of(&self.mode, &self.pipeline, key)?)))
            .collect();

        LiveReport { chunks, frontiers }
    }
}

/// The frontier of the stream or derived stream under `key`.
fn frontier_of(mode: &Live, pipeline: &Pipeline, key: &Key) -> Option<Timestamp> {
    if let Some(frontier) = mode.frontiers.get(key) {
        return Some(*frontier);
    }
    let index = pipeline
        .processors()
        .iter()
        .position(|processor| writes(processor, key) && !processor.is_transform())?;
    Some(mode.nodes[index].recipe.frontier())
}

/// The first instant an event of `sequence` starts at.
fn earliest(sequence: &Sequence) -> Option<Timestamp> {
    match sequence {
        Sequence::TimePoint(points) => points
            .instants_in(Timestamp::MIN, Timestamp::MAX)
            .into_iter()
            .min(),
        Sequence::TimeInterval(timeline) => timeline.iter().map(TimeIntervalEvent::start).min(),
    }
}
