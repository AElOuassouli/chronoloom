//! A set of formulas, each filed under the key of the member it produces.

use core::fmt;
use std::collections::BTreeMap;

use crate::store::{Key, Sequence, Store};

use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;
use super::group::Group;
use super::input::Input;
use crate::operator::compile_recipe;

/// What the members of a processor's result are: a timeline, points, or one
/// timeline per entity. In a serialised document the first two are named
/// `state` and `events`.
///
/// The last three group the `Value` records of the member [`Group::key`] by the
/// field [`Group::path`], and evaluate the formula once per entity with the
/// patterns over that member restricted to the entity's records; [`Group`]
/// explains entities, member keys and what the formula reads.
///
/// - `PerEntity { by, formula }` files one timeline per entity, under
///   `<output key>/<entity>`.
/// - `AnyEntity { by, formula }` is one timeline under the output key: active
///   wherever some entity is, known wherever one is active or all are known.
/// - `CountEntities { by, formula }` is one `TimePointSequence<i64>` under the
///   output key, the number of entities active, as its changes: a point at the
///   start of each run of the extent where every entity is known, and one at
///   each later instant the count differs from the one before. At an instant
///   the spans that end come before those that start, so a hand-over counts
///   once. Read it with `While` as a step, like any step value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Recipe {
    /// A timeline, as the formula describes it.
    #[cfg_attr(feature = "serde", serde(rename = "state"))]
    Timeline(Formula),
    /// A point sequence.
    #[cfg_attr(feature = "serde", serde(rename = "events"))]
    Points(Events),
    /// One timeline per entity, under `<output key>/<entity>`.
    PerEntity {
        /// The records and the field that holds the entity.
        by: Group,
        /// The state of one entity.
        formula: Formula,
    },
    /// The union of the timelines of the entities.
    AnyEntity {
        /// The records and the field that holds the entity.
        by: Group,
        /// The state of one entity.
        formula: Formula,
    },
    /// The number of entities active, as its changes.
    CountEntities {
        /// The records and the field that holds the entity.
        by: Group,
        /// The state of one entity.
        formula: Formula,
    },
}

impl Recipe {
    /// One timeline per entity of `by`, each of `formula`.
    #[must_use]
    pub const fn per_entity(by: Group, formula: Formula) -> Self {
        Self::PerEntity { by, formula }
    }

    /// The union of the timelines of `formula` for the entities of `by`.
    #[must_use]
    pub const fn any_entity(by: Group, formula: Formula) -> Self {
        Self::AnyEntity { by, formula }
    }

    /// How many entities of `by` are in the state `formula`, as changes.
    #[must_use]
    pub const fn count_entities(by: Group, formula: Formula) -> Self {
        Self::CountEntities { by, formula }
    }

    /// The sequence the output denotes over the members of `system`.
    ///
    /// # Errors
    ///
    /// As the formula and the events;
    /// [`ProcessError::SeveralMembers`] for `PerEntity`, which has no single
    /// sequence (add its processor to a stream system); for the grouped outputs, the errors of
    /// reading the records of the group like a projection does, and those of
    /// the formula.
    pub(crate) fn evaluate(&self, system: &Store) -> Result<Sequence, ProcessError> {
        compile_recipe(self).batch(system)
    }

    /// Every key the output reads, with repeats.
    pub(crate) fn read_keys<'a>(&'a self, keys: &mut Vec<&'a Key>) {
        match self {
            Self::Timeline(formula) => formula.read_keys(keys),
            Self::Points(events) => events.read_keys(keys),
            Self::PerEntity { by, formula }
            | Self::AnyEntity { by, formula }
            | Self::CountEntities { by, formula } => {
                keys.push(&by.key);
                formula.read_keys(keys);
            }
        }
    }
}

impl From<Formula> for Recipe {
    fn from(formula: Formula) -> Self {
        Self::Timeline(formula)
    }
}

impl From<Events> for Recipe {
    fn from(events: Events) -> Self {
        Self::Points(events)
    }
}

impl fmt::Display for Recipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeline(formula) => fmt::Display::fmt(formula, f),
            Self::Points(events) => fmt::Display::fmt(events, f),
            Self::PerEntity { by, formula } => write!(f, "{formula} per {by}"),
            Self::AnyEntity { by, formula } => write!(f, "any({formula}) per {by}"),
            Self::CountEntities { by, formula } => write!(f, "count({formula}) per {by}"),
        }
    }
}

/// The members of `system` are what the declared `inputs` say, none being an
/// output.
pub(crate) fn check_declared(
    inputs: &BTreeMap<Key, Input>,
    is_output: impl Fn(&Key) -> bool,
    system: &Store,
) -> Result<(), ProcessError> {
    for (key, input) in inputs {
        if is_output(key) {
            return Err(ProcessError::InputIsOutput { key: key.clone() });
        }
        let sequence = system
            .get(key.as_str())
            .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?;
        input.check(key, sequence)?;
    }

    Ok(())
}
