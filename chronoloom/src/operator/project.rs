//! `Project`: one field of the record values of a member, as points of a type.

use crate::primitives::TimePointEvent;
use crate::processing::{Kind, ProcessError};
use crate::sequence_system::{AnyPointSequence, Key, PointValue, SequenceSystem};
use crate::sequences::attribute::Attribute;
use crate::sequences::TimePointSequence;
use crate::values::Value;

use super::reading::records;

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

    pub(super) fn batch(&self, inputs: &SequenceSystem) -> Result<AnyPointSequence, ProcessError> {
        let records = records(&self.key, inputs)?;

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
