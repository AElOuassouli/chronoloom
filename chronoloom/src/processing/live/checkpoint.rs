//! The state of a live processor as plain data, and the checks that make one a
//! live processor again. The document that carries it is in the
//! [`serialisation`](crate::serialisation) module.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::primitives::{TimePointEvent, Timestamp};
use crate::processing::{Output, Processor};
use crate::sequence_system::{Key, MemberParts, Metadata, Sequence, SequenceSystem};
use crate::sequences::TimePointSequence;
use crate::values::Value;

use super::{group, leaves, Aggregation, Leaf, LiveProcessor, Role};

/// What a checkpoint holds beyond the processor itself.
#[derive(Serialize, Deserialize)]
pub(crate) struct Checkpoint {
    pub(crate) processor: Processor,
    /// The live inputs, and the views of the records of each entity.
    pub(crate) inputs: SequenceSystem,
    /// The inputs that have no value type yet, which the system cannot hold.
    #[serde(default)]
    pub(crate) untyped: Vec<Untyped>,
    pub(crate) outputs: Vec<OutputState>,
    #[serde(default)]
    pub(crate) groups: Vec<GroupRecord>,
    #[serde(default)]
    pub(crate) counts: Vec<CountRecord>,
    pub(crate) finished: bool,
}

/// An input that no event has given a type yet.
pub(crate) struct Untyped {
    pub(crate) key: Key,
    pub(crate) frontier: Timestamp,
    pub(crate) sequence: TimePointSequence<()>,
}

/// Where one output stands.
#[derive(Serialize, Deserialize)]
pub(crate) struct OutputState {
    pub(crate) key: Key,
    pub(crate) through: Timestamp,
    pub(crate) trusted: Timestamp,
    /// The instant of each `Between` leaf, in the order of the formula.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) cuts: Vec<Timestamp>,
}

/// The entities of one per-entity output, in the order they were seen.
#[derive(Serialize, Deserialize)]
pub(crate) struct GroupRecord {
    pub(crate) key: Key,
    pub(crate) entities: Vec<EntityRecord>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct EntityRecord {
    pub(crate) entity: Value,
    pub(crate) first: Timestamp,
}

/// The points of a count as last evaluated.
#[derive(Serialize, Deserialize)]
pub(crate) struct CountRecord {
    pub(crate) key: Key,
    pub(crate) points: Vec<TimePointEvent<i64>>,
}

/// The views of the `Between` leaves of an output, in the order of its formula.
fn between_views(output: &Output) -> Vec<Key> {
    leaves(output)
        .into_iter()
        .filter_map(|(leaf, _)| match leaf {
            Leaf::Between { start, .. } => Some(start.key.clone()),
            _ => None,
        })
        .collect()
}

impl LiveProcessor {
    /// The state to store: what cannot be recomputed from the processor.
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        let mut inputs = self.inputs.clone();
        let mut untyped = Vec::new();
        for key in &self.untyped {
            let points = self.inputs.points::<()>(key.as_str());
            let frontier = self.inputs.frontier_of(key.as_str());
            if let (Some(points), Some(frontier)) = (points, frontier) {
                inputs.remove(key.as_str());
                untyped.push(Untyped {
                    key: key.clone(),
                    frontier,
                    sequence: points.clone(),
                });
            }
        }

        let outputs = self
            .outputs
            .iter()
            .map(|(key, output)| OutputState {
                key: key.clone(),
                through: self.through[key],
                trusted: self.trusted[key],
                cuts: between_views(output)
                    .iter()
                    .map(|view| self.betweens[view].cut)
                    .collect(),
            })
            .collect();
        let groups = self
            .groups
            .iter()
            .map(|group| {
                let mut born: Vec<_> = group.entities.values().collect();
                born.sort_by_key(|born| born.index);
                GroupRecord {
                    key: group.key.clone(),
                    entities: born
                        .into_iter()
                        .map(|born| EntityRecord {
                            entity: born.value.clone(),
                            first: born.first,
                        })
                        .collect(),
                }
            })
            .collect();
        let counts = self
            .counts
            .iter()
            .map(|(key, points)| CountRecord {
                key: key.clone(),
                points: points.clone(),
            })
            .collect();

        Checkpoint {
            processor: self.processor.clone(),
            inputs,
            untyped,
            outputs,
            groups,
            counts,
            finished: self.finished,
        }
    }

    /// A live processor in the state of a checkpoint, once every check passes.
    pub(crate) fn from_checkpoint(checkpoint: Checkpoint) -> Result<Self, String> {
        let Checkpoint {
            processor,
            inputs,
            untyped,
            outputs,
            groups,
            counts,
            finished,
        } = checkpoint;

        let mut live = processor
            .live(0)
            .map_err(|error| format!("the processor cannot run online: {error}"))?;
        live.restore_entities(groups)?;
        live.restore_inputs(inputs, untyped, finished)?;
        live.restore_outputs(outputs, finished)?;
        live.restore_counts(counts)?;
        live.finished = finished;
        if !finished {
            live.check_frontiers()?;
        }

        Ok(live)
    }

    /// File the entities of each group, in the order they were seen.
    fn restore_entities(&mut self, groups: Vec<GroupRecord>) -> Result<(), String> {
        if groups.len() != self.groups.len() {
            return Err(format!(
                "the checkpoint has {} per-entity outputs and its processor {}",
                groups.len(),
                self.groups.len()
            ));
        }

        for (index, record) in groups.into_iter().enumerate() {
            let key = self.groups[index].key.clone();
            if record.key != key {
                return Err(format!(
                    "the per-entity output `{}` is where `{key}` belongs",
                    record.key
                ));
            }
            for entity in record.entities {
                let id = group::entity_id(&entity.entity).ok_or_else(|| {
                    format!("`{key}` has an entity that is not a bool, a number or text")
                })?;
                if self.groups[index].entities.contains_key(&id) {
                    return Err(format!(
                        "`{key}` has the entity {} twice",
                        group::render(&entity.entity)
                    ));
                }
                self.file_entity(index, id, entity.entity, entity.first);
            }
        }

        self.rebuild();
        let keys: BTreeSet<&Key> = self.outputs.iter().map(|(key, _)| key).collect();
        if keys.len() != self.outputs.len() {
            return Err("an entity is filed under the key of another output".to_owned());
        }

        Ok(())
    }

    /// Take the inputs, once they are the ones the processor declares.
    fn restore_inputs(
        &mut self,
        mut inputs: SequenceSystem,
        untyped: Vec<Untyped>,
        finished: bool,
    ) -> Result<(), String> {
        if finished {
            if inputs.keys().next().is_some() || !untyped.is_empty() {
                return Err("a finished processor holds no inputs".to_owned());
            }
            self.inputs = SequenceSystem::new();
            self.untyped.clear();
            return Ok(());
        }

        let named: BTreeSet<&Key> = untyped.iter().map(|input| &input.key).collect();
        if named.len() != untyped.len()
            || named.iter().any(|key| inputs.get(key.as_str()).is_some())
        {
            return Err("an input is held twice".to_owned());
        }

        // What the processor declares, plus the view of each entity.
        let mut expected: BTreeSet<Key> = self.inputs.keys().cloned().collect();
        expected.extend(
            self.groups
                .iter()
                .flat_map(|group| group.entities.values().map(|born| born.view.clone())),
        );
        let found: BTreeSet<Key> = inputs
            .keys()
            .cloned()
            .chain(untyped.iter().map(|input| input.key.clone()))
            .collect();
        if let Some(key) = expected.symmetric_difference(&found).next() {
            let problem = if found.contains(key) {
                "is not an input of the processor"
            } else {
                "is missing"
            };
            return Err(format!("the input `{key}` {problem}"));
        }

        for (key, declared) in self.inputs.iter() {
            if untyped.iter().any(|input| input.key == *key) {
                if !self.untyped.contains(key) {
                    return Err(format!(
                        "the input `{key}` has a type, and is held without one"
                    ));
                }
                continue;
            }
            let stored = inputs.get(key.as_str()).expect("every key was found above");
            match (declared, stored) {
                (Sequence::TimeInterval(_), Sequence::TimeInterval(_)) => {}
                (Sequence::TimePoint(declared), Sequence::TimePoint(stored)) => {
                    let typed = !self.untyped.contains(key);
                    if typed && declared.value_type_name() != stored.value_type_name() {
                        return Err(format!(
                            "the input `{key}` holds `{}` values, and its processor reads `{}`",
                            stored.value_type_name(),
                            declared.value_type_name()
                        ));
                    }
                }
                _ => {
                    return Err(format!(
                        "the input `{key}` is of another shape than the processor reads"
                    ))
                }
            }
        }
        for (key, _) in inputs.iter() {
            let key = key.as_str();
            if inputs.frontier_of(key).is_none() {
                return Err(format!("the input `{key}` is not live"));
            }
            if inputs.lineage(key).is_some() || inputs.lineage_entity(key).is_some() {
                return Err(format!(
                    "the input `{key}` has a lineage or an entity, which a live input has not"
                ));
            }
            // A live input carries the schema its processor declares for it, and
            // no other: the one a fresh live processor would give it.
            if inputs.schema(key) != self.inputs.schema(key) {
                return Err(format!(
                    "the input `{key}` holds another schema than its processor declares"
                ));
            }
        }

        self.check_views(&inputs)?;

        self.untyped = untyped.iter().map(|input| input.key.clone()).collect();
        for input in untyped {
            inputs.insert_parts(MemberParts {
                key: input.key,
                sequence: Sequence::from(input.sequence),
                metadata: Metadata::new(),
                frontier: Some(input.frontier),
                lineage: None,
                entity: None,
                schema: None,
            });
        }
        self.inputs = inputs;

        Ok(())
    }

    /// Each view is of the entity it stands for, and as far on as its records.
    fn check_views(&self, inputs: &SequenceSystem) -> Result<(), String> {
        for group in &self.groups {
            let by = group.by.key.as_str();
            let frontier = inputs.frontier_of(by);
            for (id, born) in &group.entities {
                let key = born.view.as_str();
                if inputs.frontier_of(key) != frontier {
                    return Err(format!(
                        "the view `{key}` of an entity is not at the frontier of `{by}`"
                    ));
                }
                let points = inputs
                    .points::<Value>(key)
                    .ok_or_else(|| format!("the view `{key}` does not hold records"))?;
                for event in points {
                    let named = group::entity_of(event.value(), &group.by.path);
                    if named.map(|(found, _)| found).as_ref() != Some(id) {
                        return Err(format!(
                            "the view `{key}` holds a record of another entity, at {}",
                            event.timestamp()
                        ));
                    }
                    if event.timestamp() < born.first {
                        return Err(format!(
                            "the view `{key}` holds a record at {}, before its entity was first seen at {}",
                            event.timestamp(),
                            born.first
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Take where each output stands.
    fn restore_outputs(&mut self, outputs: Vec<OutputState>, finished: bool) -> Result<(), String> {
        let mut states: BTreeMap<Key, OutputState> = BTreeMap::new();
        for state in outputs {
            let key = state.key.clone();
            if states.insert(key.clone(), state).is_some() {
                return Err(format!("the output `{key}` is stored twice"));
            }
        }

        self.through.clear();
        self.trusted.clear();
        let outputs = self.outputs.clone();
        for (key, output) in &outputs {
            let state = states
                .remove(key)
                .ok_or_else(|| format!("the output `{key}` has no state"))?;
            if state.trusted > state.through {
                return Err(format!(
                    "the output `{key}` is trusted from {}, past the {} it has emitted up to",
                    state.trusted, state.through
                ));
            }
            if finished && state.through != Timestamp::MAX {
                return Err(format!(
                    "the output `{key}` is not final in a finished processor"
                ));
            }
            let views = between_views(output);
            if state.cuts.len() != views.len() {
                return Err(format!(
                    "the output `{key}` has {} cuts for {} `Between` leaves",
                    state.cuts.len(),
                    views.len()
                ));
            }
            for (view, cut) in views.iter().zip(&state.cuts) {
                if *cut > state.trusted {
                    return Err(format!(
                        "the output `{key}` cuts at {cut}, past where it is trusted from"
                    ));
                }
                if let Some(between) = self.betweens.get_mut(view) {
                    between.cut = *cut;
                }
            }
            self.through.insert(key.clone(), state.through);
            self.trusted.insert(key.clone(), state.trusted);
        }
        if let Some(key) = states.keys().next() {
            return Err(format!("the output `{key}` is not one of the processor"));
        }

        Ok(())
    }

    fn restore_counts(&mut self, counts: Vec<CountRecord>) -> Result<(), String> {
        self.counts.clear();
        for record in counts {
            let counted = matches!(
                self.roles.get(&record.key),
                Some(Role::Aggregate(group))
                    if self.groups[*group].aggregation == Aggregation::Count
            );
            if !counted {
                return Err(format!("`{}` is not a count of entities", record.key));
            }
            if record
                .points
                .windows(2)
                .any(|pair| pair[0].timestamp() > pair[1].timestamp())
            {
                return Err(format!(
                    "the points of `{}` are not in time order",
                    record.key
                ));
            }
            if self
                .counts
                .insert(record.key.clone(), record.points)
                .is_some()
            {
                return Err(format!("the count `{}` is stored twice", record.key));
            }
        }

        Ok(())
    }

    /// No output has emitted past what its inputs allow: the lag rule over the
    /// frontiers of the inputs gives at least the instant each has emitted up
    /// to, as it did when the state was last advanced. Members and unions can
    /// stand below it for an entity seen since, so they are left out.
    fn check_frontiers(&self) -> Result<(), String> {
        let mut frontiers: BTreeMap<Key, Timestamp> = BTreeMap::new();
        for (key, output) in &self.outputs {
            let mut frontier = self.output_frontier(output, &frontiers);
            if let Some(cap) = self.cap(key) {
                frontier = frontier.min(cap);
            }
            let through = self.through[key];
            let free = matches!(self.roles.get(key), None | Some(Role::Template(_)));
            if free && through > frontier {
                return Err(format!(
                    "the output `{key}` has emitted up to {through}, past its frontier {frontier}"
                ));
            }
            frontiers.insert(key.clone(), frontier.max(through));
        }

        Ok(())
    }
}
