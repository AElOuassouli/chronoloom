//! One timeline per entity: the grouping of a member of records by a field.

use core::fmt;

use crate::sequence_system::Key;
use crate::values::Value;

#[cfg(doc)]
use super::formula::Formula;

/// The member of `Value` records that carries the entities, and the field of
/// each record that says which entity it is about: `Group::new("gate",
/// "agent")` groups the records of `gate` by their `agent` field.
///
/// A [`Group`] is what the per-entity outputs of a [`Processor`] are grouped
/// by: [`Recipe::PerEntity`], [`Recipe::AnyEntity`] and
/// [`Recipe::CountEntities`]. Presence is the model: an agent enters, an agent
/// exits, and what is wanted is one timeline per agent, whether any agent is
/// present, and how many are. The same shape covers sessions per user and
/// uptime per host.
///
/// ```
/// use chronoloom::{Condition, Formula, Group, Recipe, Pattern, Processor, Value};
/// use chronoloom::{StreamSystem, TimePointEvent, TimePointSequence};
///
/// let crossing = |agent: i64, direction: &str| {
///     Value::record([("agent", Value::from(agent)), ("direction", Value::from(direction))])
/// };
/// let mut result = StreamSystem::new();
/// result.insert_stream("gate", TimePointSequence::from_events("gate", vec![
///     TimePointEvent::new(10, crossing(7, "in")),
///     TimePointEvent::new(12, crossing(9, "in")),
///     TimePointEvent::new(20, crossing(7, "out")),
///     TimePointEvent::new(30, crossing(9, "out")),
/// ]))?;
///
/// // Present from an "in" of an agent until the next "out" of the same agent.
/// let direction = |text: &'static str| Pattern::field("gate", "direction", Condition::Equals(Value::from(text)));
/// let present = Formula::between(direction("in"), direction("out"));
/// let by_agent = Group::new("gate", "agent");
///
/// result.add_processor(Processor::new("presence", Recipe::per_entity(by_agent.clone(), present.clone())))?;
/// result.add_processor(Processor::new("anyone", Recipe::any_entity(by_agent.clone(), present.clone())))?;
/// result.add_processor(Processor::new("crowd", Recipe::count_entities(by_agent, present)))?;
///
/// // One timeline per agent, keyed `<output>/<agent>`.
/// let seven = result.intervals("presence/7").unwrap();
/// assert_eq!(seven[0].bounds(), (10, 20));
/// assert_eq!(result.intervals("presence/9").unwrap()[0].bounds(), (12, 30));
///
/// // Whether anyone is present, and how many are, as changes of a count.
/// assert_eq!(result.intervals("anyone").unwrap()[0].bounds(), (10, 30));
/// let crowd: Vec<(i64, i64)> = result
///     .points::<i64>("crowd")
///     .unwrap()
///     .iter()
///     .map(|change| (change.timestamp(), *change.value()))
///     .collect();
/// assert_eq!(&crowd[1..], [(10, 1), (12, 2), (20, 1), (30, 0)]);
/// # Ok::<(), chronoloom::stream_system::UpdateError>(())
/// ```
///
/// # Entities
///
/// An entity is a distinct value of the field at `path` (a dotted path, as
/// [`Value::get`] reads it) among the records of the member. Only a boolean, a
/// number or text names an entity. A record with no such field, with `Null`
/// there, with a record there, or with a `NaN` there, belongs to no entity: it
/// is not part of any entity's events, so it neither opens nor closes anything,
/// and it is never a sample of a `While`. Values are compared as numbers
/// compare, so the integer `5` and the float `5.0` are one entity, named after
/// whichever came first in time.
///
/// # Reading inside the formula
///
/// The formula of a per-entity output is evaluated once per entity. The
/// patterns it has over the member under `key` read only that entity's records,
/// as if the member had been filtered by hand and kept its extent. Patterns over
/// any other member are not restricted: a door shared by every agent stays the
/// same door for each of them. A state read with [`Formula::State`], or an
/// output of the same processor, is shared as well.
///
/// An entity exists from its first record on. Its timeline is observed from
/// that instant, so nothing is said about the entity before it, and
/// [`Recipe::AnyEntity`] and [`Recipe::CountEntities`] count it from that
/// instant too: an entity that has not appeared yet is inactive and adds no
/// uncertainty. This is what lets a live stream system learn entities as they
/// arrive and still give what the batch evaluation gives. It changes nothing
/// for a formula that is inactive before the first record of the entity, such
/// as a `Between` of its records, which is what a presence is.
///
/// # Member keys
///
/// The member of an entity is filed under `<output key>/<entity>`. Text is
/// written as it is, without quotes, and any other value as [`Value`]'s
/// `Display` writes it (`7`, `1.5`, `true`). A `/` inside the text is kept: the
/// entity is everything after the first `<output key>/`, so `agent/7` under
/// `presence` is the member `presence/agent/7`. Two entities that would be filed
/// under one key (the text `5` and the integer `5`) are
/// [`ProcessError::DuplicateMember`](super::ProcessError::DuplicateMember), and so is an entity whose member key is
/// the key of another output.
///
/// # Reading the members
///
/// Another formula of the same processor may read a member by its key,
/// `Formula::state("presence/7")`, and then reads the output, not a member of
/// the system. The dependency order treats any key under `<output key>/` as a
/// read of the whole per-entity output, which therefore comes first, and a
/// formula of the output that reads such a key is a [`ProcessError::Cycle`](super::ProcessError::Cycle).
/// A key of an entity that does not exist is [`ProcessError::UnknownKey`](super::ProcessError::UnknownKey).
/// The outputs [`Recipe::AnyEntity`] and [`Recipe::CountEntities`] are read by
/// their own key, like any output.
///
/// [`Recipe::AnyEntity`]: super::Recipe::AnyEntity
/// [`Recipe::CountEntities`]: super::Recipe::CountEntities
/// [`Recipe::PerEntity`]: super::Recipe::PerEntity
/// [`Processor`]: crate::processor::Processor
/// [`Value::get`]: crate::Value::get
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Group {
    /// The member of `Value` points whose records carry the entities.
    pub key: Key,
    /// The dotted path of the field that holds the entity.
    pub path: String,
}

impl Group {
    /// The records of the member under `key`, grouped by the field at `path`.
    #[must_use]
    pub fn new(key: impl Into<Key>, path: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            path: path.into(),
        }
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.key)
        } else {
            write!(f, "{}.{}", self.key, self.path)
        }
    }
}

/// What tells entities apart: equal values are one entity, numbers compare as
/// numbers, and the order is what the entities are listed in for ties.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntityId {
    Bool(bool),
    Integer(i64),
    /// The bits of a float that is no integer.
    Float(u64),
    Text(String),
}

/// What tells the entity named by `value` apart, if the value names one.
pub(crate) fn entity_id(value: &Value) -> Option<EntityId> {
    // 2^63, the first float above every integer.
    const BOUND: f64 = 9_223_372_036_854_775_808.0;

    Some(match value {
        Value::Bool(flag) => EntityId::Bool(*flag),
        Value::Integer(number) => EntityId::Integer(*number),
        Value::Float(number) if number.is_nan() => return None,
        Value::Float(number) if number.fract() == 0.0 && (-BOUND..BOUND).contains(number) => {
            // In range and whole, so the cast is exact.
            EntityId::Integer(*number as i64)
        }
        Value::Float(number) => EntityId::Float(number.to_bits()),
        Value::Text(text) => EntityId::Text(text.clone()),
        Value::Null | Value::Record(_) => return None,
    })
}

/// The entity a record is about, and the value naming it.
pub(crate) fn entity_of<'a>(record: &'a Value, path: &str) -> Option<(EntityId, &'a Value)> {
    let value = record.get(path)?;
    Some((entity_id(value)?, value))
}

/// How an entity is written in a member key.
pub(crate) fn render(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        other => other.to_string(),
    }
}

/// The key of the member of the entity named `name` under `output`.
pub(crate) fn member_key(output: &Key, name: &str) -> Key {
    Key::from(format!("{output}/{name}"))
}

/// Whether `key` is a member key under the output `output`.
pub(crate) fn is_under(key: &Key, output: &Key) -> bool {
    key.as_str()
        .strip_prefix(output.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
}
