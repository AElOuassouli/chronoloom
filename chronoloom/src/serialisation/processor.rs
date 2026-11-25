//! A [`Processor`] as a document: its output key, its declared inputs in key
//! order, and its formula.

use std::collections::BTreeSet;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::processing::{Input, Recipe};
use crate::processor::Processor;
use crate::store::Key;

use super::{Document, Format};

#[derive(Serialize)]
struct InputRef<'a> {
    key: &'a Key,
    input: &'a Input,
}

#[derive(Serialize)]
pub(super) struct ProcessorBodyRef<'a> {
    output: &'a Key,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<InputRef<'a>>,
    formula: &'a Recipe,
}

#[derive(Deserialize)]
struct InputBody {
    key: Key,
    input: Input,
}

#[derive(Deserialize)]
pub(super) struct ProcessorBody {
    output: Key,
    #[serde(default)]
    inputs: Vec<InputBody>,
    formula: Recipe,
}

impl<'a> ProcessorBodyRef<'a> {
    pub(super) fn of(processor: &'a Processor) -> Self {
        Self {
            output: processor.output(),
            inputs: processor
                .declared_inputs()
                .iter()
                .map(|(key, input)| InputRef { key, input })
                .collect(),
            formula: processor.recipe(),
        }
    }
}

impl ProcessorBody {
    /// The processor, checked as [`Processor::check`] does.
    pub(super) fn build(self) -> Result<Processor, String> {
        let mut declared = BTreeSet::new();
        let mut processor = Processor::new(self.output, self.formula);
        for InputBody { key, input } in self.inputs {
            if !declared.insert(key.clone()) {
                return Err(format!(
                    "the input `{key}` is declared twice in the processor"
                ));
            }
            processor = processor.with_input(key, input);
        }
        processor.check().map_err(|error| error.to_string())?;

        Ok(processor)
    }
}

impl Serialize for Processor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: ProcessorBodyRef::of(self),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Processor {
    /// Checks the processor as [`Processor::check`] does.
    ///
    /// # Errors
    ///
    /// Refuses an input declared twice, and a processor that
    /// [`Processor::check`] refuses.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<ProcessorBody>::deserialize(deserializer)?;

        body.build().map_err(de::Error::custom)
    }
}
