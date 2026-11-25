//! The live mode of a stream system: data arrives in time order, each stream
//! has a frontier before which everything is final, and the processors run
//! incrementally over what became final.

use std::collections::{BTreeMap, BTreeSet};

use crate::operator::{compile_recipe, Chunk, Inputs};
use crate::primitives::{TimeIntervalEvent, Timestamp};
use crate::processing::{check_declared, member_key, render, ProcessError, Recipe};
use crate::processor::Processor;
use crate::sequences::Extent;
use crate::store::{Key, RecordError, Sequence, Store, Tail};

#[cfg(feature = "serde")]
use super::checkpoint::Checkpoint;
use super::journal::Journal;
use super::mode::LiveNode;
use super::pipeline::{writes, Pipeline};
use super::update::Watermark;
use super::{Batch, Live, LiveReport, StreamSystem, Update, UpdateError};

impl StreamSystem<Live> {
    /// An empty system in live mode, whose streams are final before `from`:
    /// the frontier of every stream starts there, and an event before it is
    /// late.
    #[must_use]
    pub fn live(from: Timestamp) -> Self {
        Self {
            members: Store::new(),
            raw: Store::new(),
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
        next.step(
            next.mode.nodes.len() - 1,
            &mut chunks,
            &mut Journal::default(),
        )?;

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
    /// processor fails. Nothing changes in any case, also if a processor
    /// panics: the update is undone before the panic goes on.
    ///
    /// A failed update is undone from a record of what it touched (the part of
    /// each recorded stream it changed, the frontiers it moved, the processors
    /// it stepped and what it appended), so it costs what the update changes and
    /// not what the system holds.
    pub fn update(&mut self, update: Update) -> Result<LiveReport, UpdateError> {
        let mut journal = Journal::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.apply(update, &mut journal)
        }));
        match outcome {
            Ok(Ok(report)) => Ok(report),
            Ok(Err(error)) => {
                journal.undo(self);
                Err(error)
            }
            Err(panic) => {
                journal.undo(self);
                std::panic::resume_unwind(panic)
            }
        }
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

    /// End the system at `horizon`: nothing more comes, so every stream is
    /// final up to the end of time, and the processors flush what they held
    /// back (a span still open, a head not yet known).
    ///
    /// What a stream holds at or after `horizon` is dropped, and its extent
    /// stops there, so the end of what was observed is `horizon`; a stream
    /// already final past `horizon` keeps what it has. The derived streams are
    /// then what batch mode gives over the streams as they are, whole, and
    /// the report gives the last chunk of each. A finished system refuses every
    /// event as late, and a second call changes nothing.
    ///
    /// # Errors
    ///
    /// [`UpdateError::Process`] if a processor fails. Nothing changes then.
    pub fn finish(&mut self, horizon: Timestamp) -> Result<LiveReport, UpdateError> {
        let mut next = self.clone();
        let report = next.close(horizon)?;

        *self = next;
        Ok(report)
    }

    /// Take the final part of every stream and derived stream out of the
    /// system, and give it as a batch system with the same processors,
    /// metadata and schemas. The live system keeps only what is not final, so
    /// that its memory stays bounded over a stream that does not end.
    ///
    /// A span that reaches the frontier of its stream stays whole, as it may
    /// still grow, so the two halves of a stream joined end to end are the
    /// stream. Streams and derived streams keep their keys, empty when all was
    /// final. The processors do not need the drained data, as each carries its
    /// own state; a processor [added](StreamSystem::add_processor) later starts
    /// over what is still held.
    ///
    /// The derived streams of the batch system are kept as the live system made
    /// them, not recomputed from its streams: each is cut at its own frontier,
    /// which lags those of the streams, so a span may be drained after the
    /// event that started it. A derived stream is therefore not what the
    /// processors give over the drained streams alone, and an update or an
    /// added processor on the batch system evaluates over those streams and
    /// rewrites it. Keep the parts, and join them end to end, for the whole.
    #[must_use]
    pub fn drain(&mut self) -> StreamSystem<Batch> {
        let cuts: BTreeMap<Key, Timestamp> = self
            .members
            .keys()
            .filter_map(|key| Some((key.clone(), frontier_of(&self.mode, &self.pipeline, key)?)))
            .collect();

        StreamSystem {
            members: self.members.drain_before(&cuts),
            raw: Store::new(),
            pipeline: self.pipeline.clone(),
            mode: Batch,
        }
    }

    /// Where the system started: the frontier of a stream added later.
    #[cfg(feature = "serde")]
    pub(crate) const fn origin(&self) -> Timestamp {
        self.mode.from
    }

    /// The running state of each processor, in the order of the pipeline.
    #[cfg(feature = "serde")]
    pub(crate) fn nodes(&self) -> &[LiveNode] {
        &self.mode.nodes
    }

    /// A system as a checkpoint holds it: it started at `from`, holds `streams`
    /// with their frontiers, `processors` in order with the running state of
    /// each in `nodes`, and the derived streams they made, `derived`.
    ///
    /// Nothing is run: what a checkpoint holds is what the system was.
    ///
    /// # Errors
    ///
    /// If the parts contradict each other: a frontier before `from`, a
    /// processor the system refuses, a running state of another kind than its
    /// processor, a derived stream that no processor writes or that a
    /// processor lacks, entities that the running state and the derived streams
    /// do not agree on, a derived stream observed past its frontier, or a
    /// processor that has read an input further than its frontier.
    #[cfg(feature = "serde")]
    pub(crate) fn restore(parts: Checkpoint) -> Result<Self, String> {
        let Checkpoint {
            from,
            metadata,
            streams,
            processors,
            nodes,
            mut derived,
        } = parts;
        if nodes.len() != processors.len() {
            return Err(format!(
                "{} processors but the running state of {}",
                processors.len(),
                nodes.len()
            ));
        }

        let mut system = Self::live(from);
        system.metadata_mut().clone_from(&metadata);
        for stream in streams {
            if stream.frontier < from {
                return Err(format!(
                    "stream `{}`: the frontier is {}, before where the system started, {from}",
                    stream.key, stream.frontier
                ));
            }
            system.members.insert(stream.key.clone(), stream.sequence);
            system
                .mode
                .frontiers
                .insert(stream.key.clone(), stream.frontier);
            if let Some(kept) = system.members.member_metadata_mut(stream.key.as_str()) {
                *kept = stream.metadata;
            }
        }

        for (processor, node) in processors.into_iter().zip(nodes) {
            let output = processor.output().clone();
            let refused =
                |problem: &dyn std::fmt::Display| format!("processor `{output}`: {problem}");
            if processor.is_transform() {
                return Err(refused(&"a live system does not run transforms"));
            }
            let under = matches!(processor.recipe(), Recipe::PerEntity { .. })
                && system.members.keys().any(|key| writes(&processor, key));
            if system.members.contains(output.as_str()) || under {
                return Err(refused(&"its output is taken"));
            }
            system
                .pipeline
                .push(processor.clone())
                .map_err(|error| refused(&error))?;
            check_declared(
                processor.declared_inputs(),
                |key| key == &output,
                &system.members,
            )
            .map_err(|error| refused(&error))?;
            if let Some(key) = processor
                .inputs()
                .into_iter()
                .find(|key| !system.members.contains(key.as_str()))
            {
                return Err(refused(&format_args!(
                    "it reads `{key}`, which is not held"
                )));
            }

            let fresh = compile_recipe(processor.recipe()).online(&output);
            if !node.recipe.same_plan(&fresh) {
                return Err(refused(
                    &"its running state is of another kind, or holds a formula other than the \
                      processor's",
                ));
            }
            if let Some(key) = node
                .read
                .keys()
                .find(|key| !processor.inputs().contains(key))
            {
                return Err(refused(&format_args!(
                    "its state has read `{key}`, which it does not"
                )));
            }

            let (mine, rest): (Vec<_>, Vec<_>) = derived
                .into_iter()
                .partition(|member| writes(&processor, &member.key));
            derived = rest;
            let per_entity = matches!(processor.recipe(), Recipe::PerEntity { .. });
            let expected: BTreeMap<Key, Option<crate::values::Value>> = if per_entity {
                node.recipe
                    .entities()
                    .into_iter()
                    .map(|entity| (member_key(&output, &render(&entity)), Some(entity)))
                    .collect()
            } else {
                BTreeMap::from([(output.clone(), None)])
            };
            if mine.len() != expected.len()
                || mine
                    .iter()
                    .any(|member| expected.get(&member.key) != Some(&member.entity))
            {
                return Err(refused(
                    &"its derived streams disagree with its running state",
                ));
            }
            let frontier = node.recipe.frontier();
            for member in mine {
                if member.entity.is_some() != matches!(processor.recipe(), Recipe::PerEntity { .. })
                {
                    return Err(refused(&format_args!(
                        "`{}` names the wrong entity",
                        member.key
                    )));
                }
                if member
                    .sequence
                    .observed()
                    .as_slice()
                    .iter()
                    .any(|run| run.end() > frontier)
                {
                    return Err(refused(&format_args!(
                        "`{}` is observed past its frontier {frontier}",
                        member.key
                    )));
                }
                system
                    .members
                    .insert_derived(member.key.clone(), member.sequence, member.entity);
                if let Some(kept) = system.members.member_metadata_mut(member.key.as_str()) {
                    *kept = member.metadata;
                }
            }
            system.mode.nodes.push(node);
        }
        if let Some(member) = derived.first() {
            return Err(format!("`{}` is written by no processor", member.key));
        }

        for (index, node) in system.mode.nodes.iter().enumerate() {
            for (key, read) in &node.read {
                let reached = frontier_of(&system.mode, &system.pipeline, key);
                if reached.is_none_or(|reached| *read > reached) {
                    return Err(format!(
                        "processor `{}` has read `{key}` up to {read}, past its frontier",
                        system.pipeline.processors()[index].output()
                    ));
                }
            }
        }
        Ok(system)
    }

    /// [`update`](StreamSystem::update) on the system itself, which the caller
    /// undoes from `journal` if this fails.
    fn apply(&mut self, update: Update, journal: &mut Journal) -> Result<LiveReport, UpdateError> {
        let Update { chunks, watermarks } = update;
        let recorded: BTreeSet<Key> = chunks.iter().map(|(key, _)| key.clone()).collect();

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
        // A watermark that names an unknown key is refused before anything is
        // recorded or moved.
        for watermark in &watermarks {
            if let Watermark::Stream(key, _) = watermark {
                if !self.mode.frontiers.contains_key(key) {
                    return Err(if self.members.contains(key.as_str()) {
                        UpdateError::Derived { key: key.clone() }
                    } else {
                        RecordError::Unknown {
                            key: key.as_str().to_owned(),
                        }
                        .into()
                    });
                }
            }
        }

        let touched: Vec<(Key, Tail)> = chunks
            .iter()
            .filter_map(|(key, pending)| {
                let tail = self.members.tail_from(key.as_str(), pending.earliest()?)?;
                Some((key.clone(), tail))
            })
            .collect();
        for (key, tail) in touched {
            journal.recorded(key, tail);
        }
        self.members.record_update(Update {
            chunks,
            watermarks: Vec::new(),
        })?;

        // What was recorded is held to the declarations of the processors that
        // read it, as when the processor was added.
        for processor in self.pipeline.processors() {
            let output = processor.output();
            if processor
                .declared_inputs()
                .keys()
                .any(|key| recorded.contains(key))
            {
                check_declared(
                    processor.declared_inputs(),
                    |key| key == output,
                    &self.members,
                )
                .map_err(|error| UpdateError::Process {
                    processor: output.clone(),
                    error,
                })?;
            }
        }

        let mut moved: BTreeSet<Key> = BTreeSet::new();
        for watermark in watermarks {
            match watermark {
                Watermark::Stream(key, watermark) => {
                    let frontier = self
                        .mode
                        .frontiers
                        .get_mut(&key)
                        .expect("checked above to be a stream");
                    if watermark > *frontier {
                        journal.moved(key.clone(), *frontier);
                        *frontier = watermark;
                        moved.insert(key);
                    }
                }
                Watermark::All(watermark) => {
                    for (key, frontier) in &mut self.mode.frontiers {
                        if watermark > *frontier {
                            journal.moved(key.clone(), *frontier);
                            *frontier = watermark;
                            moved.insert(key.clone());
                        }
                    }
                }
            }
        }

        let mut chunks = BTreeMap::new();
        self.step_affected(&moved, &mut chunks, journal)?;
        Ok(self.report(chunks))
    }

    /// [`finish`](StreamSystem::finish) on the system itself, which the caller
    /// works on a copy of.
    fn close(&mut self, horizon: Timestamp) -> Result<LiveReport, UpdateError> {
        let mut moved: BTreeSet<Key> = BTreeSet::new();
        for (key, frontier) in &mut self.mode.frontiers {
            if *frontier == Timestamp::MAX {
                continue;
            }
            let end = horizon.max(*frontier);
            self.members
                .observe_member(key.as_str(), &Extent::of_range(&(..end)));
            *frontier = Timestamp::MAX;
            moved.insert(key.clone());
        }
        self.mode.from = Timestamp::MAX;

        let mut chunks = BTreeMap::new();
        let mut journal = Journal::default();
        self.step_affected(&moved, &mut chunks, &mut journal)?;
        Ok(self.report(chunks))
    }

    /// Step, in order, the processors that read a stream whose frontier moved
    /// (`moved`) or a derived stream whose frontier a processor before them
    /// moved. A reader of a derived stream that stayed where it was has
    /// nothing new to read, so it is not stepped.
    fn step_affected(
        &mut self,
        moved: &BTreeSet<Key>,
        chunks: &mut BTreeMap<Key, Chunk>,
        journal: &mut Journal,
    ) -> Result<(), UpdateError> {
        let affected = self.pipeline.affected(moved);
        let mut advanced: BTreeSet<usize> = BTreeSet::new();
        for index in affected {
            let reads_news = self.pipeline.processors()[index]
                .inputs()
                .into_iter()
                .any(|key| {
                    moved.contains(key)
                        || self
                            .pipeline
                            .processors()
                            .iter()
                            .position(|other| writes(other, key) && !other.is_transform())
                            .is_some_and(|producer| advanced.contains(&producer))
                });
            if !reads_news {
                continue;
            }
            let before = self.mode.nodes[index].recipe.frontier();
            self.step(index, chunks, journal)?;
            if self.mode.nodes[index].recipe.frontier() > before {
                advanced.insert(index);
            }
        }
        Ok(())
    }

    /// Run the processor at `index` over the range that became final in what
    /// it reads, and append what it gives to its derived streams. The chunks
    /// that made progress are put in `chunks`, by key. What it changes is noted
    /// in `journal`.
    fn step(
        &mut self,
        index: usize,
        chunks: &mut BTreeMap<Key, Chunk>,
        journal: &mut Journal,
    ) -> Result<(), UpdateError> {
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
        journal.stepped(index, node);
        let before = node.recipe.frontier();
        let produced = node.recipe.step(&inputs, to).map_err(failed)?;
        node.read.extend(reached);

        for member in produced {
            let (key, entity) = match member.entity {
                None => (output.clone(), None),
                Some(entity) => (member_key(output, &render(&entity)), Some(entity)),
            };
            let chunk = member.chunk;

            if self.members.contains(key.as_str()) {
                if !self.members.is_derived(key.as_str()) {
                    return Err(UpdateError::Taken { key });
                }
                // What is appended lies at or after where the processor was
                // final, so that is all an undo has to put back.
                let from = earliest(chunk.sequence()).map_or(before, |at| at.min(before));
                if let Some(tail) = self.members.tail_from(key.as_str(), from) {
                    journal.appended(key.clone(), tail);
                }
                self.members.append_derived(key.as_str(), chunk.sequence());
            } else {
                let mut sequence = chunk.clone().into_sequence();
                if let (Recipe::Timeline(formula), Sequence::TimeInterval(timeline)) =
                    (processor.recipe(), &mut sequence)
                {
                    timeline.set_attribute(formula.to_string());
                }
                self.members.insert_derived(key.clone(), sequence, entity);
                journal.created(key.clone());
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
