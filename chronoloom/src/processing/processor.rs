//! A set of formulas, each filed under the key of the member it produces.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::sequence_system::{Key, Sequence, SequenceSystem};

use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;
use super::group::{self, Derived, Group};
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
    /// As [`Formula::evaluate`] and [`Events::evaluate`];
    /// [`ProcessError::SeveralMembers`] for `PerEntity`, which has no single
    /// sequence (add its processor to a stream system); for the grouped outputs, the errors of
    /// reading the records of the group like a projection does, and those of
    /// the formula.
    pub fn evaluate(&self, system: &SequenceSystem) -> Result<Sequence, ProcessError> {
        compile_recipe(self).batch(system)
    }

    /// The members of the output filed under `key`.
    pub(crate) fn derive(
        &self,
        key: &Key,
        system: &SequenceSystem,
    ) -> Result<Vec<Derived>, ProcessError> {
        match self {
            Self::PerEntity { by, formula } => group::per_entity(key, by, formula, system),
            _ => Ok(vec![Derived {
                key: key.clone(),
                sequence: self.evaluate(system)?,
                entity: None,
            }]),
        }
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

/// Several processors as one set of outputs, each under its key, with the
/// inputs they declare: the form the evaluation and the live processor take.
/// Internal: the public recipe is [`Processor`](crate::processor::Processor),
/// one output each.
///
/// Outputs are evaluated in dependency order, an output reading another by its
/// key; one reading its own key, or a loop, is [`ProcessError::Cycle`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProcessorSet {
    outputs: BTreeMap<Key, Recipe>,
    inputs: BTreeMap<Key, Input>,
}

impl ProcessorSet {
    /// The processors of a list as one set: their outputs, and the inputs
    /// they declare.
    ///
    /// # Errors
    ///
    /// [`ProcessError::DuplicateMember`] if two processors produce one key, or
    /// declare one input as two different things.
    pub(crate) fn from_processors(
        processors: &[crate::processor::Processor],
    ) -> Result<Self, ProcessError> {
        let mut set = Self::new();
        for processor in processors {
            if set.outputs.contains_key(processor.output()) {
                return Err(ProcessError::DuplicateMember {
                    key: processor.output().clone(),
                });
            }
            set.outputs
                .insert(processor.output().clone(), processor.recipe().clone());
            for (key, input) in processor.declared_inputs() {
                match set.inputs.get(key) {
                    Some(declared) if declared != input => {
                        return Err(ProcessError::DuplicateMember { key: key.clone() });
                    }
                    _ => {
                        set.inputs.insert(key.clone(), input.clone());
                    }
                }
            }
        }
        Ok(set)
    }

    /// A processor with no outputs.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            outputs: BTreeMap::new(),
            inputs: BTreeMap::new(),
        }
    }

    /// This processor with `output` filed under `key`, replacing the output
    /// that was there.
    #[must_use]
    pub fn with(mut self, key: impl Into<Key>, output: impl Into<Recipe>) -> Self {
        self.insert(key, output);
        self
    }

    /// File `output` under `key`, returning the output that was there.
    pub fn insert(&mut self, key: impl Into<Key>, output: impl Into<Recipe>) -> Option<Recipe> {
        self.outputs.insert(key.into(), output.into())
    }

    /// The outputs, by the key of the member each produces.
    #[must_use]
    pub const fn outputs(&self) -> &BTreeMap<Key, Recipe> {
        &self.outputs
    }

    /// This processor expecting `input` of the member under `key`, replacing
    /// the declaration that was there.
    #[must_use]
    pub fn with_input(mut self, key: impl Into<Key>, input: Input) -> Self {
        self.insert_input(key, input);
        self
    }

    /// Declare that the member under `key` is `input`, returning the
    /// declaration that was there.
    pub fn insert_input(&mut self, key: impl Into<Key>, input: Input) -> Option<Input> {
        self.inputs.insert(key.into(), input)
    }

    /// The declared inputs, by key.
    #[must_use]
    pub const fn inputs(&self) -> &BTreeMap<Key, Input> {
        &self.inputs
    }

    /// Whether the outputs can be evaluated in some order: none reads itself,
    /// directly or through others. Needs no system.
    ///
    /// # Errors
    ///
    /// [`ProcessError::Cycle`] with the keys of one cycle, in key order.
    pub fn check(&self) -> Result<(), ProcessError> {
        self.order().map(drop)
    }

    /// A new system holding only the outputs, each under its key and carrying
    /// it as its lineage.
    ///
    /// # Errors
    ///
    /// The first declared input the system does not satisfy, in key order (see
    /// the set); [`ProcessError::Cycle`] if the outputs read each other
    /// in a circle, otherwise the first [`ProcessError`] any output meets, in
    /// evaluation order (key order where no output reads another).
    pub fn run(&self, system: &SequenceSystem) -> Result<SequenceSystem, ProcessError> {
        let mut produced = SequenceSystem::new();
        for (output, derived) in self.evaluate(system)? {
            for member in derived {
                produced.insert_derived(member.key, member.sequence, output.clone(), member.entity);
            }
        }

        Ok(produced)
    }

    /// The members of `system` are what the declared inputs say.
    fn check_inputs(&self, system: &SequenceSystem) -> Result<(), ProcessError> {
        for (key, input) in &self.inputs {
            if self.outputs.contains_key(key) {
                return Err(ProcessError::InputIsOutput { key: key.clone() });
            }
            let sequence = system
                .get(key.as_str())
                .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?;
            input.check(key, sequence)?;
        }

        Ok(())
    }

    /// The keys of the other outputs each output reads.
    fn dependencies(&self) -> BTreeMap<&Key, BTreeSet<&Key>> {
        self.outputs
            .iter()
            .map(|(key, output)| {
                let mut read = Vec::new();
                output.read_keys(&mut read);

                let outputs = read
                    .into_iter()
                    .flat_map(|name| {
                        // A key under a per-entity output reads all of it, and
                        // the key of any other output reads that output.
                        self.outputs.iter().filter_map(move |(named, other)| {
                            let reads = match other {
                                Recipe::PerEntity { .. } => group::is_under(name, named),
                                _ => named == name,
                            };
                            reads.then_some(named)
                        })
                    })
                    .collect();
                (key, outputs)
            })
            .collect()
    }

    /// The keys in an order where every output comes after the outputs it
    /// reads; key order among those that do not constrain each other.
    pub(super) fn order(&self) -> Result<Vec<&Key>, ProcessError> {
        enum Visit {
            OnPath,
            Done,
        }

        fn visit<'a>(
            key: &'a Key,
            dependencies: &BTreeMap<&'a Key, BTreeSet<&'a Key>>,
            visits: &mut BTreeMap<&'a Key, Visit>,
            path: &mut Vec<&'a Key>,
            order: &mut Vec<&'a Key>,
        ) -> Result<(), ProcessError> {
            match visits.get(key) {
                Some(Visit::Done) => return Ok(()),
                Some(Visit::OnPath) => {
                    let start = path.iter().position(|&on_path| on_path == key).unwrap_or(0);
                    let mut keys: Vec<Key> = path[start..].iter().map(|&k| k.clone()).collect();
                    keys.sort();
                    return Err(ProcessError::Cycle { keys });
                }
                None => {}
            }

            visits.insert(key, Visit::OnPath);
            path.push(key);
            for &read in &dependencies[key] {
                visit(read, dependencies, visits, path, order)?;
            }
            path.pop();
            visits.insert(key, Visit::Done);
            order.push(key);

            Ok(())
        }

        let dependencies = self.dependencies();
        let mut visits = BTreeMap::new();
        let mut order = Vec::with_capacity(self.outputs.len());
        for &key in dependencies.keys() {
            visit(key, &dependencies, &mut visits, &mut Vec::new(), &mut order)?;
        }

        Ok(order)
    }

    /// Every output evaluated, none inserted into `system`.
    ///
    /// Outputs that read no other output are evaluated against `system` as it
    /// is. Otherwise each is evaluated, in dependency order, against a working
    /// copy that holds the outputs evaluated so far under their keys, so a
    /// formula that names an output sees the output, not a member of the same
    /// key.
    fn evaluate(
        &self,
        system: &SequenceSystem,
    ) -> Result<Vec<(&Recipe, Vec<Derived>)>, ProcessError> {
        let order = self.order()?;
        self.check_inputs(system)?;
        let chained = self.dependencies().values().any(|read| !read.is_empty());

        let mut working = chained.then(|| system.clone());
        let mut evaluated = Vec::with_capacity(order.len());
        let mut filed: BTreeSet<Key> = BTreeSet::new();
        for key in order {
            let output = &self.outputs[key];
            let derived = output.derive(key, working.as_ref().unwrap_or(system))?;
            for member in &derived {
                if !filed.insert(member.key.clone()) {
                    return Err(ProcessError::DuplicateMember {
                        key: member.key.clone(),
                    });
                }
                if let Some(working) = &mut working {
                    working.insert(member.key.clone(), member.sequence.clone());
                }
            }
            evaluated.push((output, derived));
        }

        Ok(evaluated)
    }
}
