//! The definition of a processor.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::operator::compile_recipe;
use crate::processing::{self, Input, ProcessError, Recipe};
use crate::sequence_system::{Key, Sequence, SequenceSystem};
use crate::stream_system::{writes, Mode, StreamSystem};

/// One recipe of a [`StreamSystem`]: the inputs it reads, a formula over them,
/// and the key of the one output it produces.
///
/// The recipe is a [`Recipe`]: a state [`Formula`] gives a timeline, an
/// [`Events`] formula gives points, and a per-entity output gives the family
/// of members `<output>/<entity>`, one per entity (see [`Recipe`]).
///
/// The inputs are the keys the formula reads, found from the formula itself:
/// [`Processor::new`] needs nothing else. [`with_input`](Processor::with_input)
/// declares what one of them must be (a timeline, points of a value type,
/// records of a schema, as [`Input`] says), which the system checks every
/// time it evaluates the processor; a declared key the formula does not read
/// is an input all the same, and must be present.
///
/// # Declared inputs and validation
///
/// Declarations are optional, and a processor without any behaves as if every
/// key its formula reads were declared as what the formula reads it as. They
/// serve two purposes.
///
/// - [`validate`](Processor::validate) checks the processor against its own
///   declarations without a system, and lists every problem rather than the
///   first: a formula reading its own output ([`ProcessError::Cycle`]), then
///   its problems in the order the formula reads them (the group of a
///   per-entity output first, then the tree from left to right), each wrapped
///   in [`ProcessError::InOutput`] naming the output. Once any input is
///   declared, a read of a key that is not declared is
///   [`ProcessError::UnknownKey`]. A read that contradicts its declaration is
///   [`ProcessError::NotATimeline`], [`ProcessError::NotPoints`] or
///   [`ProcessError::ValueTypeMismatch`]; for a declared schema, a field
///   pattern, projection or group whose path the schema lacks is
///   [`ProcessError::MissingField`], a projection of a field of another kind is
///   [`ProcessError::ProjectionMismatch`], and a field pattern comparing with a
///   constant the field never has is [`ProcessError::FieldMismatch`].
///   [`check`](Processor::check) reports the first of them.
/// - Wherever the processor is evaluated, the members it reads are held to the
///   declarations first: a declared input that is missing is
///   [`ProcessError::UnknownKey`], one of another shape or value type is as
///   above, and records not of the declared schema are
///   [`ProcessError::SchemaMismatch`]. A [`LiveProcessor`] creates its inputs
///   as declared, so a record not of the declared schema is refused when it is
///   fed.
///
/// With the `serde` feature a processor is a document of its own: its
/// `output`, its declared `inputs` (left out when there are none) and its
/// `formula` (the wire name of the recipe).
///
/// [`StreamSystem`]: crate::stream_system::StreamSystem
/// [`LiveProcessor`]: crate::LiveProcessor
/// [`Formula`]: crate::processing::Formula
/// [`Events`]: crate::processing::Events
#[derive(Debug, Clone, PartialEq)]
pub struct Processor {
    output: Key,
    recipe: Recipe,
    declared: BTreeMap<Key, Input>,
}

impl Processor {
    /// A processor producing `output` from `recipe`.
    pub fn new(output: impl Into<Key>, recipe: impl Into<Recipe>) -> Self {
        Self {
            output: output.into(),
            recipe: recipe.into(),
            declared: BTreeMap::new(),
        }
    }

    /// Declare what the input under `key` must be.
    #[must_use]
    pub fn with_input(mut self, key: impl Into<Key>, input: Input) -> Self {
        self.declared.insert(key.into(), input);
        self
    }

    /// The key of the output.
    #[must_use]
    pub const fn output(&self) -> &Key {
        &self.output
    }

    /// The recipe of the output.
    #[must_use]
    pub const fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    /// Every key the processor reads: the keys of its formula and the declared
    /// inputs.
    #[must_use]
    pub fn inputs(&self) -> BTreeSet<&Key> {
        let mut read = Vec::new();
        self.recipe.read_keys(&mut read);
        read.into_iter().chain(self.declared.keys()).collect()
    }

    /// What the input under `key` was declared to be, if it was.
    #[must_use]
    pub fn declared(&self, key: &str) -> Option<&Input> {
        self.declared.get(key)
    }

    /// Every problem the processor has on its own, without a system: a
    /// formula reading its own output, a read that contradicts a declared
    /// input (another shape or value type, a field the declared schema does not
    /// have), in the order the formula reads them.
    ///
    /// # Errors
    ///
    /// Every problem found, never an empty list.
    pub fn validate(&self) -> Result<(), Vec<ProcessError>> {
        self.as_set().problems(false)
    }

    /// The first problem [`validate`](Processor::validate) would report, if
    /// any; cheaper, as it stops there.
    ///
    /// # Errors
    ///
    /// That problem.
    pub fn check(&self) -> Result<(), ProcessError> {
        self.as_set()
            .problems(true)
            .map_err(|mut problems| problems.swap_remove(0))
    }

    /// The sequence the recipe denotes over the members of `system`, once:
    /// nothing is stored, and the system is not changed.
    ///
    /// # Errors
    ///
    /// The first declared input the system does not satisfy, in key order;
    /// [`ProcessError::Cycle`] for a processor reading its own output, or a
    /// member under it, as adding it to a stream system would refuse;
    /// otherwise the first [`ProcessError`] the recipe meets. For a per-entity
    /// output that is
    /// [`ProcessError::SeveralMembers`], as it has no single sequence (add its
    /// processor to a stream system).
    pub fn evaluate<M: Mode>(&self, system: &StreamSystem<M>) -> Result<Sequence, ProcessError> {
        processing::check_declared(&self.declared, |key| key == &self.output, system.members())?;
        if self.inputs().into_iter().any(|key| writes(self, key)) {
            return Err(ProcessError::Cycle {
                keys: vec![self.output.clone()],
            });
        }
        self.recipe.evaluate(system.members())
    }

    /// The inputs whose shape was declared.
    pub(crate) const fn declared_inputs(&self) -> &BTreeMap<Key, Input> {
        &self.declared
    }

    /// The same recipe as a set of one output, which is what the live
    /// processor and the validation take.
    pub(crate) fn as_set(&self) -> processing::ProcessorSet {
        self.declared.iter().fold(
            processing::ProcessorSet::new().with(self.output.clone(), self.recipe.clone()),
            |set, (key, input)| set.with_input(key.clone(), input.clone()),
        )
    }

    /// A new system holding the members of the output, each carrying the
    /// recipe as its lineage, evaluated through the operators.
    ///
    /// # Errors
    ///
    /// The first declared input the system does not satisfy, in key order,
    /// otherwise the first [`ProcessError`] the recipe meets.
    pub(crate) fn run(&self, system: &SequenceSystem) -> Result<SequenceSystem, ProcessError> {
        processing::check_declared(&self.declared, |key| key == &self.output, system)?;

        let mut produced = SequenceSystem::new();
        for member in compile_recipe(&self.recipe).members(&self.output, system)? {
            produced.insert_derived(
                member.key,
                member.sequence,
                self.recipe.clone(),
                member.entity,
            );
        }

        Ok(produced)
    }
}

impl fmt::Display for Processor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {}", self.output, self.recipe)
    }
}
