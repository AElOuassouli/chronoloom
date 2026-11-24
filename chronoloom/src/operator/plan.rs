//! The tree a formula compiles into.

use crate::processing::{Events, Formula, ProcessError, Recipe};
use crate::sequences::TimeIntervalSequence;
use crate::store::Key;
use crate::store::{AnyPointSequence, Sequence, Store};

use super::and::And;
use super::between::Between;
use super::changes::Changes;
use super::edges::Edges;
use super::entities::{AnyEntity, CountEntities, Member, PerEntity};
use super::matching::Matching;
use super::not::Not;
use super::occurs::Occurs;
use super::online::OnlineNode;
use super::online_points::OnlinePoints;
use super::online_recipe::OnlineRecipe;
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

/// A compiled recipe: its timeline, points or entities compiled.
#[derive(Debug, Clone)]
pub(crate) enum RecipePlan {
    Timeline(TimelinePlan),
    Points(PointsPlan),
    PerEntity(PerEntity),
    AnyEntity(AnyEntity),
    CountEntities(CountEntities),
}

impl RecipePlan {
    /// The sequence of the recipe over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &Store) -> Result<Sequence, ProcessError> {
        match self {
            Self::Timeline(plan) => plan.batch(inputs).map(Sequence::from),
            Self::Points(plan) => plan.batch(inputs).map(Sequence::from),
            Self::PerEntity(_) => Err(ProcessError::SeveralMembers),
            Self::AnyEntity(operator) => operator.batch(inputs).map(Sequence::from),
            Self::CountEntities(operator) => operator.batch(inputs).map(Sequence::from),
        }
    }

    /// The running state of the recipe, whose members are filed under `output`.
    pub(crate) fn online(&self, output: &Key) -> OnlineRecipe {
        match self {
            Self::Timeline(plan) => OnlineRecipe::Timeline(plan.online()),
            Self::Points(plan) => OnlineRecipe::Points(plan.online()),
            Self::PerEntity(operator) => OnlineRecipe::PerEntity(operator.online(output)),
            Self::AnyEntity(operator) => OnlineRecipe::AnyEntity(operator.online()),
            Self::CountEntities(operator) => OnlineRecipe::CountEntities(operator.online()),
        }
    }

    /// The members of the output filed under `output`: one, or one per entity.
    pub(crate) fn members(
        &self,
        output: &Key,
        inputs: &Store,
    ) -> Result<Vec<Member>, ProcessError> {
        match self {
            Self::PerEntity(operator) => operator.members(output, inputs),
            _ => Ok(vec![Member {
                key: output.clone(),
                sequence: self.batch(inputs)?,
                entity: None,
            }]),
        }
    }
}

/// The plan of a recipe.
pub(crate) fn compile_recipe(recipe: &Recipe) -> RecipePlan {
    match recipe {
        Recipe::Timeline(formula) => RecipePlan::Timeline(TimelinePlan::compile(formula)),
        Recipe::Points(events) => RecipePlan::Points(PointsPlan::compile(events)),
        Recipe::PerEntity { by, formula } => {
            RecipePlan::PerEntity(PerEntity::new(by.clone(), formula))
        }
        Recipe::AnyEntity { by, formula } => {
            RecipePlan::AnyEntity(AnyEntity::new(recipe.to_string(), by.clone(), formula))
        }
        Recipe::CountEntities { by, formula } => {
            RecipePlan::CountEntities(CountEntities::new(recipe.to_string(), by.clone(), formula))
        }
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

    /// The formula written out, which labels the timeline.
    pub(super) fn label(&self) -> &str {
        &self.label
    }

    /// The running state of the plan.
    pub(crate) fn online(&self) -> OnlineNode {
        self.root.online()
    }

    /// The labelled timeline of the plan over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &Store) -> Result<TimeIntervalSequence, ProcessError> {
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

    /// The running state of the plan.
    pub(crate) fn online(&self) -> OnlinePoints {
        self.root.online()
    }

    /// The labelled points of the plan over the whole `inputs`.
    pub(crate) fn batch(&self, inputs: &Store) -> Result<AnyPointSequence, ProcessError> {
        self.root.batch(inputs)
    }
}

impl PointsNode {
    fn online(&self) -> OnlinePoints {
        match self {
            Self::Edges(operator) => operator.online(),
            Self::Matching(operator) => operator.online(),
            Self::Changes(operator) => operator.online(),
            Self::Project(operator) => operator.online(),
        }
    }

    fn batch(&self, inputs: &Store) -> Result<AnyPointSequence, ProcessError> {
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

    /// The running state of the node.
    pub(super) fn online(&self) -> OnlineNode {
        match self {
            Self::State(operator) => operator.online(),
            Self::Occurs(operator) => operator.online(),
            Self::Not(operator) => operator.online(),
            Self::And(operator) => operator.online(),
            Self::Or(operator) => operator.online(),
            Self::Between(operator) => operator.online(),
            Self::While(operator) => operator.online(),
            Self::Shift(operator) => operator.online(),
        }
    }

    pub(super) fn batch(&self, inputs: &Store) -> Result<TimeIntervalSequence, ProcessError> {
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
