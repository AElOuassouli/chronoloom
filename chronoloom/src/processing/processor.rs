//! A set of formulas, each filed under the key of the member it produces.

use core::fmt;
use std::collections::BTreeMap;

use crate::sequence_system::{Key, Sequence, SequenceSystem};

use super::errors::ProcessError;
use super::events::Events;
use super::formula::Formula;

/// What one member of a processor's result is: a state or events.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// A timeline, as the formula describes it.
    State(Formula),
    /// A point sequence.
    Events(Events),
}

impl Output {
    /// The sequence the output denotes over the members of `system`.
    ///
    /// # Errors
    ///
    /// As [`Formula::evaluate`] and [`Events::evaluate`].
    pub fn evaluate(&self, system: &SequenceSystem) -> Result<Sequence, ProcessError> {
        match self {
            Self::State(formula) => formula.evaluate(system).map(Sequence::from),
            Self::Events(events) => events.evaluate(system),
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
/// Outputs do not read each other: every formula is evaluated against the
/// system as it was, so the order they were added in does not matter. [`apply`]
/// evaluates all of them before it inserts any, so a failure leaves the system
/// as it was. An output replaces the member under its key, metadata included,
/// as [`SequenceSystem::insert`] does.
///
/// [`apply`]: Processor::apply
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Processor {
    outputs: BTreeMap<Key, Output>,
}

impl Processor {
    /// A processor with no outputs.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            outputs: BTreeMap::new(),
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

    /// A new system holding only the outputs, each under its key.
    ///
    /// # Errors
    ///
    /// The first [`ProcessError`] any output meets, in key order.
    pub fn run(&self, system: &SequenceSystem) -> Result<SequenceSystem, ProcessError> {
        let mut produced = SequenceSystem::new();
        for (key, sequence) in self.evaluate(system)? {
            produced.insert(key, sequence);
        }

        Ok(produced)
    }

    /// Add the outputs to `system`, replacing any member under the same key.
    ///
    /// # Errors
    ///
    /// The first [`ProcessError`] any output meets, in key order. Nothing is
    /// inserted in that case.
    pub fn apply(&self, system: &mut SequenceSystem) -> Result<(), ProcessError> {
        for (key, sequence) in self.evaluate(system)? {
            system.insert(key, sequence);
        }

        Ok(())
    }

    /// Every output evaluated against `system`, none inserted.
    fn evaluate(&self, system: &SequenceSystem) -> Result<Vec<(Key, Sequence)>, ProcessError> {
        self.outputs
            .iter()
            .map(|(key, output)| Ok((key.clone(), output.evaluate(system)?)))
            .collect()
    }
}
