//! The entity recipes: `PerEntity`, `AnyEntity` and `CountEntities`, each
//! evaluating its formula once per entity of a member of records.

use std::collections::BTreeMap;

use crate::primitives::{TimeIntervalEvent, TimePointEvent, Timestamp};
use crate::processing::{entity_of, member_key, render, EntityId, Formula, Group, ProcessError};
use crate::sequences::attribute::Attribute;
use crate::sequences::interval::operations::{intersection_spans, union_spans};
use crate::sequences::{Extent, TimeIntervalSequence, TimePointSequence};
use crate::store::{Key, Sequence, Store};
use crate::values::Value;

use super::online_entities::{OnlineAnyEntity, OnlineCountEntities, OnlinePerEntity};
use super::plan::TimelinePlan;
use super::reading::records;

/// An entity of a batch member: its records, then its timeline once the
/// formula was evaluated over them.
struct Entity {
    value: Value,
    name: String,
    /// The instant of its first record.
    first: Timestamp,
    events: Vec<TimePointEvent<Value>>,
    timeline: TimeIntervalSequence,
}

/// The entities of the records grouped by `path`, by name.
fn entities(records: &TimePointSequence<Value>, path: &str) -> Vec<(EntityId, Entity)> {
    let mut found: BTreeMap<EntityId, Entity> = BTreeMap::new();
    for event in records {
        let Some((id, value)) = entity_of(event.value(), path) else {
            continue;
        };
        found
            .entry(id)
            .or_insert_with(|| Entity {
                value: value.clone(),
                name: render(value),
                first: event.timestamp(),
                events: Vec::new(),
                timeline: TimeIntervalSequence::from_spans(String::new(), Vec::new()),
            })
            .events
            .push(event.clone());
    }

    let mut found: Vec<(EntityId, Entity)> = found.into_iter().collect();
    found.sort_by(|first, second| {
        first
            .1
            .name
            .cmp(&second.1.name)
            .then_with(|| first.0.cmp(&second.0))
    });

    found
}

/// The entities with their timelines, and the extent of the records.
struct Timelines {
    entities: Vec<Entity>,
    observed: Extent,
}

impl Timelines {
    /// The instant each entity first appeared and its timeline.
    fn members(&self) -> Vec<(Timestamp, &TimeIntervalSequence)> {
        self.entities
            .iter()
            .map(|entity| (entity.first, &entity.timeline))
            .collect()
    }
}

/// The formula evaluated for each entity, over the records restricted to it,
/// observed from its first record on.
fn timelines(by: &Group, plan: &TimelinePlan, system: &Store) -> Result<Timelines, ProcessError> {
    let records = records(&by.key, system)?;
    let observed = records.observed().clone();
    let mut entities: Vec<Entity> = entities(records, &by.path)
        .into_iter()
        .map(|(_, entity)| entity)
        .collect();

    if !entities.is_empty() {
        let mut working = system.clone();
        for entity in &mut entities {
            let view = TimePointSequence::from_parts(
                Attribute::name(by.key.as_str()),
                std::mem::take(&mut entity.events),
                observed.clone(),
            );
            working.insert(by.key.clone(), view);

            entity.timeline = plan.batch(&working)?;
            entity
                .timeline
                .observe(&Extent::of_range(&(entity.first..)));
        }
    }

    Ok(Timelines { entities, observed })
}

/// One member of a processor's result: its key, its sequence, and the entity
/// of a per-entity output.
pub(crate) struct Member {
    pub(crate) key: Key,
    pub(crate) sequence: Sequence,
    pub(crate) entity: Option<Value>,
}

/// One timeline per entity, filed under `<output>/<entity>`.
#[derive(Debug, Clone)]
pub(crate) struct PerEntity {
    by: Group,
    plan: TimelinePlan,
}

impl PerEntity {
    pub(super) fn new(by: Group, formula: &Formula) -> Self {
        Self {
            by,
            plan: TimelinePlan::compile(formula),
        }
    }

    /// The running state, with the members filed under `output`.
    pub(super) fn online(&self, output: &Key) -> OnlinePerEntity {
        OnlinePerEntity::new(
            self.plan.label().to_owned(),
            output.clone(),
            self.by.clone(),
            self.plan.online(),
        )
    }

    /// The members of the output filed under `output`.
    pub(super) fn members(
        &self,
        output: &Key,
        system: &Store,
    ) -> Result<Vec<Member>, ProcessError> {
        let found = timelines(&self.by, &self.plan, system)?;
        for pair in found.entities.windows(2) {
            if pair[0].name == pair[1].name {
                return Err(ProcessError::DuplicateMember {
                    key: member_key(output, &pair[0].name),
                });
            }
        }

        Ok(found
            .entities
            .into_iter()
            .map(|entity| Member {
                key: member_key(output, &entity.name),
                sequence: Sequence::from(entity.timeline),
                entity: Some(entity.value),
            })
            .collect())
    }
}

/// Active wherever some entity is.
#[derive(Debug, Clone)]
pub(crate) struct AnyEntity {
    label: String,
    by: Group,
    plan: TimelinePlan,
}

impl AnyEntity {
    pub(super) fn new(label: String, by: Group, formula: &Formula) -> Self {
        Self {
            label,
            by,
            plan: TimelinePlan::compile(formula),
        }
    }

    pub(super) fn online(&self) -> OnlineAnyEntity {
        OnlineAnyEntity::new(self.label.clone(), self.by.clone(), self.plan.online())
    }

    pub(super) fn batch(&self, system: &Store) -> Result<TimeIntervalSequence, ProcessError> {
        let found = timelines(&self.by, &self.plan, system)?;
        let members = found.members();

        Ok(any_of(self.label.clone(), &members, &found.observed))
    }
}

/// The number of entities active, as its changes.
#[derive(Debug, Clone)]
pub(crate) struct CountEntities {
    label: String,
    by: Group,
    plan: TimelinePlan,
}

impl CountEntities {
    pub(super) fn new(label: String, by: Group, formula: &Formula) -> Self {
        Self {
            label,
            by,
            plan: TimelinePlan::compile(formula),
        }
    }

    pub(super) fn online(&self) -> OnlineCountEntities {
        OnlineCountEntities::new(self.label.clone(), self.by.clone(), self.plan.online())
    }

    pub(super) fn batch(&self, system: &Store) -> Result<TimePointSequence<i64>, ProcessError> {
        let found = timelines(&self.by, &self.plan, system)?;
        let members = found.members();

        Ok(count_of(
            self.label.clone(),
            &members,
            &found.observed,
            Timestamp::MIN,
        ))
    }
}

/// The instants an entity's timeline is known at, which includes every instant
/// before it first appeared: an entity not seen yet says nothing, so that what
/// is known at an instant never depends on entities that appear later.
/// `known_by_all` restricts the instants before the earliest entity to the
/// records' extent, since no entity says anything there.
fn known_by(timeline: &TimeIntervalSequence, first: Timestamp) -> Vec<TimeIntervalEvent<()>> {
    if first > Timestamp::MIN {
        union_spans(
            timeline.observed().as_slice(),
            &[TimeIntervalEvent::raw(Timestamp::MIN, first)],
        )
    } else {
        timeline.observed().as_slice().to_vec()
    }
}

/// The instants every entity that has appeared is known at, and `observed`
/// before the first appeared; `observed` with no entity.
fn known_by_all(
    members: &[(Timestamp, &TimeIntervalSequence)],
    observed: &Extent,
) -> Vec<TimeIntervalEvent<()>> {
    let mut known = members
        .iter()
        .map(|(first, timeline)| known_by(timeline, *first));
    let Some(first) = known.next() else {
        return observed.as_slice().to_vec();
    };
    let all = known.fold(first, |all, next| intersection_spans(&all, &next));

    let earliest = members
        .iter()
        .map(|(first, _)| *first)
        .min()
        .unwrap_or(Timestamp::MAX);
    if earliest == Timestamp::MIN {
        return all;
    }
    let seen = union_spans(
        observed.as_slice(),
        &[TimeIntervalEvent::raw(earliest, Timestamp::MAX)],
    );
    intersection_spans(&all, &seen)
}

/// The union of the entities, known where one is active or all are known.
///
/// `members` are the instant each entity first appeared and its timeline; with
/// none the result is inactive over `observed`.
pub(crate) fn any_of(
    label: String,
    members: &[(Timestamp, &TimeIntervalSequence)],
    observed: &Extent,
) -> TimeIntervalSequence {
    let spans = members.iter().fold(Vec::new(), |all, (_, timeline)| {
        union_spans(&all, timeline.as_slice())
    });
    let known = union_spans(&known_by_all(members, observed), &spans);

    TimeIntervalSequence::from_parts(
        Attribute::name(label),
        (0, 0),
        spans,
        Extent::from_normalized(known),
    )
}

/// The number of entities active, as the points where it changes.
///
/// There is a point at the start of each run of the extent, where every entity
/// is known, whatever the count is, and one at each later instant the count
/// differs from the one before. At one instant the spans that end come before
/// those that start, so a hand-over counts once.
///
/// A count over timelines cut at `from` (a live system's window)
/// starts there; a whole one is counted from `Timestamp::MIN`.
pub(crate) fn count_of(
    label: String,
    members: &[(Timestamp, &TimeIntervalSequence)],
    observed: &Extent,
    from: Timestamp,
) -> TimePointSequence<i64> {
    let mut extent = Extent::from_normalized(known_by_all(members, observed));
    if from > Timestamp::MIN {
        extent.narrow(&Extent::of_range(&(from..)));
    }

    let mut deltas: BTreeMap<Timestamp, i64> = BTreeMap::new();
    for (_, timeline) in members {
        for span in *timeline {
            *deltas.entry(span.start()).or_insert(0) += 1;
            *deltas.entry(span.end()).or_insert(0) -= 1;
        }
    }
    let deltas: Vec<(Timestamp, i64)> = deltas.into_iter().collect();

    let mut events = Vec::new();
    let mut running = 0;
    let mut cursor = 0;
    for run in extent.as_slice() {
        let (start, end) = run.bounds();

        while cursor < deltas.len() && deltas[cursor].0 <= start {
            running += deltas[cursor].1;
            cursor += 1;
        }
        events.push(TimePointEvent::new(start, running));

        let mut last = running;
        while cursor < deltas.len() && deltas[cursor].0 < end {
            running += deltas[cursor].1;
            if running != last {
                events.push(TimePointEvent::new(deltas[cursor].0, running));
                last = running;
            }
            cursor += 1;
        }
    }

    TimePointSequence::from_parts(Attribute::name(label), events, extent)
}
