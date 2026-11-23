//! The tree a formula compiles into.

use crate::processing::{EntityRecipe, Events, Formula, ProcessError, Recipe};
use crate::sequence_system::{AnyPointSequence, Sequence, SequenceSystem};
use crate::sequences::TimeIntervalSequence;

use super::and::And;
use super::between::Between;
use super::changes::Changes;
use super::edges::Edges;
use super::matching::Matching;
use super::not::Not;
use super::occurs::Occurs;
use super::or::Or;
use super::project::Project;
use super::r#while::While;
use super::shift::Shift;
use super::state::State;

/// A compiled formula: its timeline, labelled with the formula written out.
#[derive(Debug, Clone)]
pub(crate) struct TimelinePlan {
    root: Node,
    label: String,
}

/// Compiled events.
#[derive(Debug, Clone)]
pub(crate) struct PointsPlan {
    root: PointsNode,
}

/// One node of a plan whose result is a timeline.
#[derive(Debug, Clone)]
pub(super) enum Node {
    State(State),
    Occurs(Occurs),
    Not(Not),
    And(And),
    Or(Or),
    Between(Between),
    While(While),
    Shift(Shift),
}

/// The root of a plan whose result is points.
#[derive(Debug, Clone)]
enum PointsNode {
    Edges(Edges),
    Matching(Matching),
    Changes(Changes),
    Project(Project),
}

/// A compiled recipe: its timeline or points compiled, the entity outputs left
/// to the evaluation they had before.
#[derive(Debug, Clone)]
pub(crate) enum RecipePlan {
    Timeline(TimelinePlan),
    Points(PointsPlan),
    /// `PerEntity`, `AnyEntity` and `CountEntities`, which the operators do not
    /// own yet.
    Entity(EntityRecipe),
}

impl RecipePlan {
    /// The sequence of the recipe over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &SequenceSystem) -> Result<Sequence, ProcessError> {
        match self {
            Self::Timeline(plan) => plan.batch(inputs).map(Sequence::from),
            Self::Points(plan) => plan.batch(inputs).map(Sequence::from),
            Self::Entity(recipe) => recipe.evaluate(inputs),
        }
    }
}

/// The plan of a recipe.
pub(crate) fn compile_recipe(recipe: &Recipe) -> RecipePlan {
    match recipe {
        Recipe::Timeline(formula) => RecipePlan::Timeline(TimelinePlan::compile(formula)),
        Recipe::Points(events) => RecipePlan::Points(PointsPlan::compile(events)),
        Recipe::PerEntity { .. } => RecipePlan::Entity(EntityRecipe::PerEntity),
        Recipe::AnyEntity { by, formula } => RecipePlan::Entity(EntityRecipe::AnyEntity {
            label: recipe.to_string(),
            by: by.clone(),
            formula: formula.clone(),
        }),
        Recipe::CountEntities { by, formula } => RecipePlan::Entity(EntityRecipe::CountEntities {
            label: recipe.to_string(),
            by: by.clone(),
            formula: formula.clone(),
        }),
    }
}

impl TimelinePlan {
    /// The plan of a formula.
    pub(crate) fn compile(formula: &Formula) -> Self {
        Self {
            root: Node::compile(formula),
            label: formula.to_string(),
        }
    }

    /// The labelled timeline of the plan over the whole `inputs`.
    pub(crate) fn batch(
        &self,
        inputs: &SequenceSystem,
    ) -> Result<TimeIntervalSequence, ProcessError> {
        let mut timeline = self.root.batch(inputs)?;
        timeline.set_attribute(self.label.clone());
        Ok(timeline)
    }
}

impl PointsPlan {
    /// The plan of events.
    pub(crate) fn compile(events: &Events) -> Self {
        let label = events.to_string();
        let root = match events {
            Events::Edges(formula) => PointsNode::Edges(Edges::new(Node::compile(formula), label)),
            Events::Matching(pattern) => {
                PointsNode::Matching(Matching::new(pattern.clone(), label))
            }
            Events::Changes { key } => PointsNode::Changes(Changes::new(key.clone(), label)),
            Events::Project { key, path, into } => {
                PointsNode::Project(Project::new(key.clone(), path.clone(), *into, label))
            }
        };

        Self { root }
    }

    /// The labelled points of the plan over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        self.root.batch(inputs)
    }
}

impl PointsNode {
    fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        match self {
            Self::Edges(operator) => operator.batch(inputs),
            Self::Matching(operator) => operator.batch(inputs),
            Self::Changes(operator) => operator.batch(inputs),
            Self::Project(operator) => operator.batch(inputs),
        }
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
            Formula::Between { start, end, open } => {
                Self::Between(Between::new(start.clone(), end.clone(), *open))
            }
            Formula::While { pattern, timeout } => {
                Self::While(While::new(pattern.clone(), *timeout))
            }
            Formula::Shift {
                formula,
                alpha,
                beta,
            } => Self::Shift(Shift::new(Self::compile(formula), *alpha, *beta)),
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
            Self::Between(operator) => operator.batch(inputs),
            Self::While(operator) => operator.batch(inputs),
            Self::Shift(operator) => operator.batch(inputs),
        }
    }
}
