//! The online form of a recipe: the running state of whatever a processor
//! compiled into, whether it makes one member or one per entity.

use crate::primitives::Timestamp;
use crate::processing::ProcessError;
use crate::values::Value;

use super::chunk::{Chunk, Inputs};
use super::online::OnlineNode;
use super::online_entities::{OnlineAnyEntity, OnlineCountEntities, OnlinePerEntity};
use super::online_points::OnlinePoints;

/// The chunk of one member of a recipe's output, and the entity it is about
/// for a per-entity output.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct MemberChunk {
    pub(crate) entity: Option<Value>,
    pub(crate) chunk: Chunk,
}

/// The running state of a recipe.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum OnlineRecipe {
    Timeline(OnlineNode),
    Points(OnlinePoints),
    PerEntity(OnlinePerEntity),
    AnyEntity(OnlineAnyEntity),
    CountEntities(OnlineCountEntities),
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineRecipe {
    /// The chunk of every member over the newly final range, the new frontier
    /// being at most `to`.
    ///
    /// A per-entity output gives one chunk per entity seen so far; the chunk of
    /// an entity first seen in this step starts at the start of time. Every
    /// other recipe gives one member, with no entity.
    ///
    /// # Errors
    ///
    /// If a member the recipe reads is missing from `inputs`, or is of another
    /// shape or value type than it reads, or two entities share a name.
    pub(crate) fn step(
        &mut self,
        inputs: &Inputs,
        to: Timestamp,
    ) -> Result<Vec<MemberChunk>, ProcessError> {
        let single = |chunk| {
            vec![MemberChunk {
                entity: None,
                chunk,
            }]
        };

        Ok(match self {
            Self::Timeline(node) => single(node.step(inputs, to)?),
            Self::Points(node) => single(node.step(inputs, to)?),
            Self::AnyEntity(node) => single(node.step(inputs, to)?),
            Self::CountEntities(node) => single(node.step(inputs, to)?),
            Self::PerEntity(node) => node
                .step(inputs, to)?
                .into_iter()
                .map(|member| MemberChunk {
                    entity: Some(member.entity),
                    chunk: member.chunk,
                })
                .collect(),
        })
    }

    /// How far the output is final.
    pub(crate) fn frontier(&self) -> Timestamp {
        match self {
            Self::Timeline(node) => node.frontier(),
            Self::Points(node) => node.frontier(),
            Self::PerEntity(node) => node.frontier(),
            Self::AnyEntity(node) => node.frontier(),
            Self::CountEntities(node) => node.frontier(),
        }
    }
}
