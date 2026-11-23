//! The tree a formula compiles into.

use crate::primitives::Timestamp;
use crate::processing::{between, hold};
use crate::processing::{Formula, OpenEnd, Pattern, ProcessError, Recipe};
use crate::sequence_system::{Sequence, SequenceSystem};
use crate::sequences::TimeIntervalSequence;

use super::and::And;
use super::not::Not;
use super::occurs::Occurs;
use super::or::Or;
use super::state::State;

/// A compiled formula.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    root: Node,
}

/// One node of a plan.
#[derive(Debug, Clone)]
pub(super) enum Node {
    State(State),
    Occurs(Occurs),
    Not(Not),
    And(And),
    Or(Or),
    Legacy(Legacy),
}

/// The nodes whose evaluation has not moved into an operator yet.
#[derive(Debug, Clone)]
pub(super) enum Legacy {
    Between {
        start: Pattern,
        end: Pattern,
        open: OpenEnd,
    },
    While {
        pattern: Pattern,
        timeout: Option<Timestamp>,
    },
    Shift {
        inner: Box<Node>,
        alpha: Timestamp,
        beta: Timestamp,
    },
}

/// A compiled recipe: its timeline compiled, the other outputs left to the
/// evaluation they had before.
#[derive(Debug, Clone)]
pub(crate) enum RecipePlan {
    Timeline(Plan, String),
    /// `Points`, `PerEntity`, `AnyEntity` and `CountEntities`.
    Legacy(Recipe),
}

impl RecipePlan {
    /// The sequence of the recipe over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &SequenceSystem) -> Result<Sequence, ProcessError> {
        match self {
            Self::Timeline(plan, label) => {
                let mut timeline = plan.batch(inputs)?;
                timeline.set_attribute(label.clone());
                Ok(Sequence::from(timeline))
            }
            Self::Legacy(recipe) => recipe.evaluate_legacy(inputs),
        }
    }
}

impl Plan {
    /// The plan of a recipe.
    pub(crate) fn compile_recipe(recipe: &Recipe) -> RecipePlan {
        match recipe {
            Recipe::Timeline(formula) => {
                RecipePlan::Timeline(Self::compile(formula), formula.to_string())
            }
            other => RecipePlan::Legacy(other.clone()),
        }
    }

    /// The plan of a formula.
    pub(crate) fn compile(formula: &Formula) -> Self {
        Self {
            root: Node::compile(formula),
        }
    }

    /// The timeline of the plan over the whole `inputs`, unlabelled.
    pub(crate) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        self.root.batch(inputs)
    }
}

impl Node {
    fn compile(formula: &Formula) -> Self {
        match formula {
            Formula::State(key) => Self::State(State::new(key.clone())),
            Formula::Occurs(pattern) => Self::Occurs(Occurs::new(pattern.clone())),
            Formula::Not(inner) => Self::Not(Not::new(Self::compile(inner))),
            Formula::And(formulas) => {
                Self::And(And::new(formulas.iter().map(Self::compile).collect()))
            }
            Formula::Or(formulas) => {
                Self::Or(Or::new(formulas.iter().map(Self::compile).collect()))
            }
            Formula::Between { start, end, open } => Self::Legacy(Legacy::Between {
                start: start.clone(),
                end: end.clone(),
                open: *open,
            }),
            Formula::While { pattern, timeout } => Self::Legacy(Legacy::While {
                pattern: pattern.clone(),
                timeout: *timeout,
            }),
            Formula::Shift {
                formula,
                alpha,
                beta,
            } => Self::Legacy(Legacy::Shift {
                inner: Box::new(Self::compile(formula)),
                alpha: *alpha,
                beta: *beta,
            }),
        }
    }

    pub(super) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        match self {
            Self::State(operator) => operator.batch(inputs),
            Self::Occurs(operator) => operator.batch(inputs),
            Self::Not(operator) => operator.batch(inputs),
            Self::And(operator) => operator.batch(inputs),
            Self::Or(operator) => operator.batch(inputs),
            Self::Legacy(Legacy::Between { start, end, open }) => {
                between(start, end, *open, inputs)
            }
            Self::Legacy(Legacy::While { pattern, timeout }) => hold(pattern, *timeout, inputs),
            Self::Legacy(Legacy::Shift { inner, alpha, beta }) => {
                Ok(inner.batch(inputs)?.transform_unbounded(*alpha, *beta))
            }
        }
    }
}
