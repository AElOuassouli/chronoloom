//! `Project`: one field of the record values of a member, as points of a type.

use crate::primitives::{TimePointEvent, Timestamp};
use crate::processing::{Kind, ProcessError};
use crate::sequence_system::{AnyPointSequence, Key, PointValue, Sequence, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::TimePointSequence;
use crate::values::Value;

use super::chunk::{Chunk, Inputs};
use super::online_points::OnlinePoints;
use super::reading::{records, records_of};

#[derive(Debug, Clone)]
pub(super) struct Project {
    key: Key,
    path: String,
    into: Kind,
    label: String,
}

impl Project {
    pub(super) const fn new(key: Key, path: String, into: Kind, label: String) -> Self {
        Self {
            key,
            path,
            into,
            label,
        }
    }

    pub(super) fn online(&self) -> OnlinePoints {
        OnlinePoints::Project(OnlineProject {
            project: self.clone(),
            frontier: Timestamp::MIN,
        })
    }

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        self.read(records(&self.key, inputs)?)
    }

    /// The field of `records` as points of the type asked for.
    fn read(&self, records: &TimePointSequence<Value>) -> Result<AnyPointSequence, ProcessError> {
        match self.into {
            Kind::Bool => self.collect(records, Value::as_bool),
            Kind::Integer => self.collect(records, Value::as_integer),
            Kind::Float => self.collect(records, Value::as_float),
            Kind::Text => self.collect(records, |value| value.as_text().map(str::to_owned)),
            Kind::Value => self.collect(records, |value| Some(value.clone())),
        }
    }

    /// The points whose field `convert` reads, skipping the records without it.
    fn collect<T: PointValue>(
        &self,
        records: &TimePointSequence<Value>,
        convert: impl Fn(&Value) -> Option<T>,
    ) -> Result<AnyPointSequence, ProcessError> {
        let mut events = Vec::new();
        for event in records {
            let field = match event.value().get(&self.path) {
                None => continue,
                Some(Value::Null) if self.into != Kind::Value => continue,
                Some(field) => field,
            };
            let converted = convert(field).ok_or_else(|| ProcessError::ProjectionMismatch {
                key: self.key.clone(),
                path: self.path.clone(),
                expected: self.into,
                found: field.kind_name(),
            })?;
            events.push(TimePointEvent::new(event.timestamp(), converted));
        }

        Ok(AnyPointSequence::from(TimePointSequence::from_parts(
            Attribute::name(self.label.clone()),
            events,
            records.observed().clone(),
        )))
    }
}

/// The running state of [`Project`]: each chunk of records read on its own.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct OnlineProject {
    project: Project,
    frontier: Timestamp,
}

#[cfg_attr(not(test), allow(dead_code))]
impl OnlineProject {
    pub(super) fn step(&mut self, inputs: &Inputs, to: Timestamp) -> Result<Chunk, ProcessError> {
        let key = &self.project.key;
        let chunk = inputs
            .get(key)
            .ok_or_else(|| ProcessError::UnknownKey { key: key.clone() })?
            .slice(self.frontier, to);
        let records = records_of(key, Some(chunk.sequence()))?;
        let projected = self.project.read(records)?;

        self.frontier = chunk.to();
        Ok(Chunk::new(
            chunk.from(),
            chunk.to(),
            Sequence::TimePoint(projected),
        ))
    }

    pub(super) const fn frontier(&self) -> Timestamp {
        self.frontier
    }
}
