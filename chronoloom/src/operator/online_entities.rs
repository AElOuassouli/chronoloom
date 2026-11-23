//! The online form of the entity recipes: one online state of the formula per
//! entity, the points of each entity routed to its own.

use std::collections::{BTreeMap, BTreeSet};

use crate::primitives::{TimePointEvent, Timestamp};
use crate::processing::{entity_of, member_key, render, EntityId, Group, ProcessError};
use crate::sequence_system::{Key, Sequence};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::operations::union_spans;
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use crate::values::Value;

use super::chunk::{Chunk, Inputs};
use super::entities::{any_of, count_of};
use super::online::OnlineNode;
use super::reading::records_of;

/// An entity seen: what names it, the instant of its first record, and the
/// online state of the formula over its records.
#[derive(Debug, Clone)]
struct Born {
    value: Value,
    name: String,
    first: Timestamp,
    state: OnlineNode,
    /// What the state gave beyond the frontier of the records, from the
    /// frontier of the output on.
    held: Option<Chunk>,
}

/// What one step of the entities gives: the range it covers, the extent the
/// records were observed over in it, and the chunk of each entity.
struct Advance {
    from: Timestamp,
    to: Timestamp,
    observed: Extent,
    entities: Vec<Stepped>,
}

/// The chunk of one entity over the range of a step.
struct Stepped {
    id: EntityId,
    value: Value,
    name: String,
    first: Timestamp,
    /// Whether the entity first appeared in this step.
    born: bool,
    /// Its timeline over the range, unobserved before `first`.
    timeline: Chunk,
}

/// The entities of a group and the prototype they are born from.
///
/// The output stops at the frontier of the records, which is where a record of
/// an entity not seen yet could still appear: what the states give beyond it is
/// held back.
///
/// Before its first record an entity has none, so the state of the formula over
/// an entity that has not appeared is the state over no record at all: the
/// prototype, which is stepped over empty records. An entity seen mid-stream is
/// a copy of the prototype from then on, and its timeline is unobserved before
/// its first record.
#[derive(Debug, Clone)]
struct Population {
    by: Group,
    /// The output a duplicate member of a per-entity output is reported under.
    unique_under: Option<Key>,
    prototype: OnlineNode,
    /// What the prototype gave beyond the output, as `Born::held`.
    held: Option<Chunk>,
    entities: BTreeMap<EntityId, Born>,
    /// What the records were observed over, from the frontier on.
    observed: Extent,
    frontier: Timestamp,
}

impl Population {
    fn new(by: Group, unique_under: Option<Key>, prototype: OnlineNode) -> Self {
        Self {
            by,
            unique_under,
            prototype,
            held: None,
            entities: BTreeMap::new(),
            observed: Extent::nothing(),
            frontier: Timestamp::MIN,
        }
    }

    /// The records of the group over the range of `chunk`, as a chunk of
    /// `events`.
    fn view(&self, chunk: &Chunk, events: Vec<TimePointEvent<Value>>, observed: &Extent) -> Chunk {
        let points = TimePointSequence::from_parts(
            Attribute::name(self.by.key.as_str()),
            events,
            observed.clone(),
        );
        Chunk::new(chunk.from(), chunk.to(), Sequence::from(points))
    }

    /// Step every entity, and the prototype, over the newly final range.
    fn advance(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Advance, ProcessError> {
        let key = &self.by.key;
        let chunk = inputs
            .get(key)
            .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?;
        let records = records_of(key, Some(chunk.sequence()))?;
        let reached = chunk.to();

        let mut routed: BTreeMap<EntityId, Vec<TimePointEvent<Value>>> = BTreeMap::new();
        let mut newcomers: BTreeSet<EntityId> = BTreeSet::new();
        for event in records {
            let Some((id, value)) = entity_of(event.value(), &self.by.path) else {
                continue;
            };

            if !self.entities.contains_key(&id) {
                let name = render(value);
                if let Some(output) = &self.unique_under {
                    if self.entities.values().any(|born| born.name == name) {
                        return Err(ProcessError::DuplicateMember {
                            key: member_key(output, &name),
                        });
                    }
                }
                self.entities.insert(
                    id.clone(),
                    Born {
                        value: value.clone(),
                        name,
                        first: event.timestamp(),
                        state: self.prototype.clone(),
                        held: self.held.clone(),
                    },
                );
                newcomers.insert(id.clone());
            }
            routed.entry(id).or_default().push(event.clone());
        }

        let observed = records.observed().clone();
        self.observed =
            Extent::from_normalized(union_spans(self.observed.as_slice(), observed.as_slice()));

        let from = self.frontier;
        let views: Vec<(EntityId, Chunk)> = self
            .entities
            .keys()
            .map(|id| {
                let events = routed.remove(id).unwrap_or_default();
                (id.clone(), self.view(chunk, events, &observed))
            })
            .collect();
        for (id, view) in &views {
            let mut local = inputs.clone();
            local.insert(self.by.key.clone(), view.clone());
            let born = self
                .entities
                .get_mut(id)
                .expect("the entity was listed above");
            let given = born.state.step(&local, to)?;
            born.held = Some(match born.held.take() {
                Some(held) => held.join(given),
                None => given,
            });
        }

        let mut local = inputs.clone();
        local.insert(self.by.key.clone(), self.view(chunk, Vec::new(), &observed));
        let given = self.prototype.step(&local, to)?;
        let held = match self.held.take() {
            Some(held) => held.join(given),
            None => given,
        };

        let limit = held.to().min(reached).max(from);
        self.held = Some(held.slice(limit, held.to()));
        self.frontier = limit;

        let mut stepped = Vec::with_capacity(self.entities.len());
        for (id, born) in &mut self.entities {
            let held = born.held.take().expect("the entity was stepped above");
            let mut timeline = held
                .slice(from, limit)
                .timeline()
                .expect("online nodes give timelines")
                .clone();
            timeline.observe(&Extent::of_range(&(born.first..)));
            born.held = Some(held.slice(limit, held.to()));
            stepped.push(Stepped {
                born: newcomers.contains(id),
                id: id.clone(),
                value: born.value.clone(),
                name: born.name.clone(),
                first: born.first,
                timeline: Chunk::new(from, limit, Sequence::from(timeline)),
            });
        }

        let mut within = self.observed.clone();
        within.narrow(&Extent::of_range(&(from..limit)));
        self.observed.narrow(&Extent::of_range(&(limit..)));

        Ok(Advance {
            from,
            to: limit,
            observed: within,
            entities: stepped,
        })
    }
}

/// One timeline per entity, as the chunks of the entities.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlinePerEntity {
    label: String,
    population: Population,
}

/// The chunk of the timeline of an entity, from the start of time on for an
/// entity that first appeared in the step.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct EntityChunk {
    pub(crate) entity: Value,
    pub(crate) chunk: Chunk,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlinePerEntity {
    pub(super) fn new(label: String, output: Key, by: Group, prototype: OnlineNode) -> Self {
        Self {
            label,
            population: Population::new(by, Some(output), prototype),
        }
    }

    /// The chunk of every entity seen so far, ordered by name.
    ///
    /// The chunk of an entity that first appeared in this step starts at the
    /// start of time: before its first record nothing is observed of it. The
    /// chunks of the others continue theirs.
    ///
    /// # Errors
    ///
    /// If the records are missing from `inputs`, are of another shape or value
    /// type, or two entities have the same name.
    pub(crate) fn step(
        &mut self,
        inputs: &Inputs,
        to: Timestamp,
    ) -> Result<Vec<EntityChunk>, ProcessError> {
        let mut advance = self.population.advance(inputs, to)?;
        advance
            .entities
            .sort_by(|first, second| (&first.name, &first.id).cmp(&(&second.name, &second.id)));

        Ok(advance
            .entities
            .into_iter()
            .map(|stepped| {
                let mut chunk = stepped.timeline;
                if stepped.born && chunk.from() > Timestamp::MIN {
                    let nothing = TimeIntervalSequence::from_spans(self.label.clone(), Vec::new())
                        .with_observed(Extent::nothing());
                    let before = Chunk::new(Timestamp::MIN, chunk.from(), Sequence::from(nothing));
                    chunk = before.join(chunk);
                }
                EntityChunk {
                    entity: stepped.value,
                    chunk,
                }
            })
            .collect())
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.population.frontier
    }
}

/// Active wherever some entity is.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlineAnyEntity {
    label: String,
    population: Population,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineAnyEntity {
    pub(super) fn new(label: String, by: Group, prototype: OnlineNode) -> Self {
        Self {
            label,
            population: Population::new(by, None, prototype),
        }
    }

    /// # Errors
    ///
    /// If the records are missing from `inputs`, or are of another shape or
    /// value type.
    pub(crate) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let advance = self.population.advance(inputs, to)?;
        let members = members_of(&advance);
        let any = any_of(self.label.clone(), &members, &advance.observed);

        Ok(Chunk::new(advance.from, advance.to, Sequence::from(any)))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.population.frontier
    }
}

/// The number of entities active, as its changes.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlineCountEntities {
    label: String,
    population: Population,
    /// The count the last chunk ended on, if its extent reached its end: a
    /// first point of the next chunk with the same count is no change.
    carried: Option<i64>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineCountEntities {
    pub(super) fn new(label: String, by: Group, prototype: OnlineNode) -> Self {
        Self {
            label,
            population: Population::new(by, None, prototype),
            carried: None,
        }
    }

    /// # Errors
    ///
    /// If the records are missing from `inputs`, or are of another shape or
    /// value type.
    pub(crate) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let advance = self.population.advance(inputs, to)?;
        let members = members_of(&advance);
        let counted = count_of(
            self.label.clone(),
            &members,
            &advance.observed,
            Timestamp::MIN,
        );

        let mut events: Vec<TimePointEvent<i64>> = counted.iter().cloned().collect();
        if let (Some(first), Some(carried)) = (events.first(), self.carried) {
            if first.timestamp() == advance.from && *first.value() == carried {
                events.remove(0);
            }
        }
        if advance.from < advance.to {
            let reaches = counted
                .observed()
                .as_slice()
                .last()
                .is_some_and(|run| run.bounds().1 == advance.to);
            self.carried = if reaches {
                counted.iter().last().map(|event| *event.value())
            } else {
                None
            };
        }

        let points = TimePointSequence::from_parts(
            Attribute::name(self.label.as_str()),
            events,
            counted.observed().clone(),
        );
        Ok(Chunk::new(advance.from, advance.to, Sequence::from(points)))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.population.frontier
    }
}

/// The instant each entity first appeared and its timeline over the step.
fn members_of(advance: &Advance) -> Vec<(Timestamp, &TimeIntervalSequence)> {
    advance
        .entities
        .iter()
        .map(|stepped| {
            let timeline = stepped
                .timeline
                .timeline()
                .expect("online nodes give timelines");
            (stepped.first, timeline)
        })
        .collect()
}
