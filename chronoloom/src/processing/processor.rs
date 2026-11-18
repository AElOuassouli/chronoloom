//! A set of formulas, each filed under the key of the member it produces.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::sequence_system::{Key, Sequence, SequenceSystem};

use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;
use super::group::{self, Derived, Group};
use super::input::Input;

/// What the members of a processor's result are: a state, events, or one
/// timeline per entity.
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
pub enum Output {
    /// A timeline, as the formula describes it.
    State(Formula),
    /// A point sequence.
    Events(Events),
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

impl Output {
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
    /// sequence (run a [`Processor`]); for the grouped outputs, the errors of
    /// reading the records of the group like a projection does, and those of
    /// the formula.
    pub fn evaluate(&self, system: &SequenceSystem) -> Result<Sequence, ProcessError> {
        match self {
            Self::State(formula) => formula.evaluate(system).map(Sequence::from),
            Self::Events(events) => events.evaluate(system),
            Self::PerEntity { .. } => Err(ProcessError::SeveralMembers),
            Self::AnyEntity { by, formula } => {
                group::any(self.to_string(), by, formula, system).map(Sequence::from)
            }
            Self::CountEntities { by, formula } => {
                group::count(self.to_string(), by, formula, system).map(Sequence::from)
            }
        }
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
    pub(super) fn read_keys<'a>(&'a self, keys: &mut Vec<&'a Key>) {
        match self {
            Self::State(formula) => formula.read_keys(keys),
            Self::Events(events) => events.read_keys(keys),
            Self::PerEntity { by, formula }
            | Self::AnyEntity { by, formula }
            | Self::CountEntities { by, formula } => {
                keys.push(&by.key);
                formula.read_keys(keys);
            }
        }
    }
}

impl From<Formula> for Output {
    fn from(formula: Formula) -> Self {
        Self::State(formula)
    }
}

impl From<Events> for Output {
    fn from(events: Events) -> Self {
        Self::Events(events)
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(formula) => fmt::Display::fmt(formula, f),
            Self::Events(events) => fmt::Display::fmt(events, f),
            Self::PerEntity { by, formula } => write!(f, "{formula} per {by}"),
            Self::AnyEntity { by, formula } => write!(f, "any({formula}) per {by}"),
            Self::CountEntities { by, formula } => write!(f, "count({formula}) per {by}"),
        }
    }
}

/// A set of formulas that turns the members of a system into new ones.
///
/// Each output is filed under a [`Key`] and says what that member is, as a
/// [`Formula`] (a timeline) or [`Events`] (points). Turning point sequences
/// into a timeline, a timeline into edges, or several states into one all go
/// through a processor, so the sequence types hold only data, extent and the
/// set algebra. A processor is data too: it can be compared, cloned and
/// extended, and it is the same value whatever system it is run against.
///
/// ```
/// use chronoloom::{Condition, Formula, Pattern, Processor, SequenceSystem};
/// use chronoloom::{TimePointEvent, TimePointSequence};
///
/// let mut room = SequenceSystem::new();
/// room.insert("count", TimePointSequence::from_events("count", vec![
///     TimePointEvent::new(0, 3_i64),
///     TimePointEvent::new(10, 8),
///     TimePointEvent::new(20, 4),
/// ]));
///
/// let crowded = Formula::while_matching(Pattern::new("count", Condition::Greater(5_i64)));
/// let processor = Processor::new().with("crowded", crowded);
///
/// // `run` makes a system of the outputs alone, `apply` adds them to the input.
/// let derived = processor.run(&room)?;
/// assert_eq!(derived.intervals("crowded").map(|s| s.len()), Some(1));
///
/// processor.apply(&mut room)?;
/// assert!(room.contains("count") && room.contains("crowded"));
/// # Ok::<(), chronoloom::ProcessError>(())
/// ```
///
/// # Chained outputs
///
/// A formula may name the key of another output of the same processor, and
/// then reads that output, not a member of the system: an output wins over a
/// member under the same key, inside the processor. Outputs are evaluated in
/// dependency order, so "present while motion or door" can read the output
/// "motion" without a second processor, and the order they were added in does
/// not matter. A formula that reads its own key, or a loop of outputs, is a
/// [`ProcessError::Cycle`], reported by [`run`], [`apply`] and [`check`] before
/// anything is evaluated.
///
/// # Applying
///
/// [`apply`] evaluates all outputs before it inserts any, so a failure leaves
/// the system as it was. An output replaces the member under its key, metadata
/// included, as [`SequenceSystem::insert`] does, and the output is kept as the
/// member's lineage; see [`SequenceSystem::lineage`].
///
/// # Declared inputs and validation
///
/// A processor can say what it expects of the members it reads, with
/// [`with_input`](Processor::with_input): an [`Input`] per key, a timeline or
/// points of one value type, and for records the [`Schema`] of their values.
/// Declarations are optional, and a processor without any behaves as it
/// always did. They serve three purposes.
///
/// - [`validate`] checks the processor against its own declarations without a
///   system, and lists every problem rather than the first. It reports, in this
///   order: the cycle, if any, as [`check`] does; each declared input that is
///   also an output ([`ProcessError::InputIsOutput`]), in key order; then, for
///   each output in key order, its problems in the order its formula reads
///   them (the group of a per-entity output first, then the tree from left to
///   right), each wrapped in [`ProcessError::InOutput`] naming the output. A
///   read of a key that is neither an output nor a declared input is
///   [`ProcessError::UnknownKey`] once any input is declared. A read that
///   contradicts the declaration is [`ProcessError::NotATimeline`],
///   [`ProcessError::NotPoints`] or [`ProcessError::ValueTypeMismatch`], and
///   for a declared schema, a field pattern, projection or group whose path the
///   schema lacks is [`ProcessError::MissingField`], a projection of a field of
///   another kind is [`ProcessError::ProjectionMismatch`], and a field
///   pattern comparing with a constant the field never has is
///   [`ProcessError::FieldMismatch`]. Reads of outputs are not checked, and
///   members of a per-entity output are known only when their entities are, so
///   no duplicate member key is known statically.
/// - [`check`] stays the cheap test, cycles only: when it fails, the first
///   error of `validate` is the same error.
/// - [`run`] and [`apply`] check the system against the declarations before
///   anything is evaluated: a declared input the system lacks is
///   [`ProcessError::UnknownKey`], one of another shape or value type is as
///   above, and records not of the declared schema are
///   [`ProcessError::SchemaMismatch`]. [`Processor::live`] makes the inputs it
///   creates as declared, so a read that contradicts a declaration is the same
///   error, an input only declared is created too, and a record not of the
///   declared schema is refused when it is fed.
///
/// With the `serde` feature the declarations are part of the processor
/// document, in an `inputs` array that is left out when empty.
///
/// [`Schema`]: crate::Schema
/// [`apply`]: Processor::apply
/// [`check`]: Processor::check
/// [`run`]: Processor::run
/// [`validate`]: Processor::validate
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Processor {
    outputs: BTreeMap<Key, Output>,
    inputs: BTreeMap<Key, Input>,
}

impl Processor {
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
    pub fn with(mut self, key: impl Into<Key>, output: impl Into<Output>) -> Self {
        self.insert(key, output);
        self
    }

    /// File `output` under `key`, returning the output that was there.
    pub fn insert(&mut self, key: impl Into<Key>, output: impl Into<Output>) -> Option<Output> {
        self.outputs.insert(key.into(), output.into())
    }

    /// The outputs, by the key of the member each produces.
    #[must_use]
    pub const fn outputs(&self) -> &BTreeMap<Key, Output> {
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
    /// [`Processor`]); [`ProcessError::Cycle`] if the outputs read each other
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

    /// Add the outputs to `system`, replacing any member under the same key,
    /// and record each output as the lineage of the member it wrote.
    ///
    /// # Errors
    ///
    /// As [`run`](Processor::run). Nothing is inserted in that case.
    pub fn apply(&self, system: &mut SequenceSystem) -> Result<(), ProcessError> {
        for (output, derived) in self.evaluate(system)? {
            for member in derived {
                system.insert_derived(member.key, member.sequence, output.clone(), member.entity);
            }
        }

        Ok(())
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
                                Output::PerEntity { .. } => group::is_under(name, named),
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
    ) -> Result<Vec<(&Output, Vec<Derived>)>, ProcessError> {
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
