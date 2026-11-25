//! Processors run in batch, the way the tests before stream systems ran a set
//! of outputs: a stream system holding the streams of another and the
//! processors, each added after the outputs it reads.

#![allow(dead_code, reason = "each test file uses some of these")]

use chronoloom::stream_system::{StreamSystem, UpdateError};
use chronoloom::{
    Events, Formula, ProcessError, Processor, Recipe, Sequence, TimeIntervalSequence,
};

/// A batch stream system of `streams` and `processors`, or the error of the
/// first processor that cannot be added once every other that can has been.
pub fn run(streams: &StreamSystem, processors: &[Processor]) -> Result<StreamSystem, ProcessError> {
    let mut system = streams.clone();

    let mut pending = processors.to_vec();
    while !pending.is_empty() {
        let mut first_error = None;
        let mut index = 0;
        while index < pending.len() {
            match system.add_processor(pending[index].clone()) {
                Ok(_) => {
                    pending.remove(index);
                    first_error = None;
                    index = 0;
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                    index += 1;
                }
            }
        }
        if let Some(error) = first_error {
            return Err(match error {
                UpdateError::Process { error, .. } => error,
                UpdateError::Taken { key } => ProcessError::DuplicateMember { key },
                other => panic!("an unexpected refusal: {other}"),
            });
        }
    }
    Ok(system)
}

/// A recipe evaluated once over a stream system, through
/// [`Processor::evaluate`].
pub trait Evaluate {
    /// What the recipe evaluates to.
    type Output;

    /// The output over `system`, nothing stored.
    fn evaluate_over(&self, system: &StreamSystem) -> Result<Self::Output, ProcessError>;
}

impl Evaluate for Recipe {
    type Output = Sequence;

    fn evaluate_over(&self, system: &StreamSystem) -> Result<Sequence, ProcessError> {
        Processor::new("output", self.clone()).evaluate(system)
    }
}

impl Evaluate for Events {
    type Output = Sequence;

    fn evaluate_over(&self, system: &StreamSystem) -> Result<Sequence, ProcessError> {
        Recipe::Points(self.clone()).evaluate_over(system)
    }
}

impl Evaluate for Formula {
    type Output = TimeIntervalSequence;

    fn evaluate_over(&self, system: &StreamSystem) -> Result<TimeIntervalSequence, ProcessError> {
        let sequence = Recipe::Timeline(self.clone()).evaluate_over(system)?;
        Ok(sequence
            .as_time_interval()
            .expect("a formula is a timeline")
            .clone())
    }
}
