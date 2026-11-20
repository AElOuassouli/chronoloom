//! A [`Processor`] as a document: its output key, its declared inputs in key
//! order, and its formula.

use std::collections::BTreeSet;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::processing::{Input, Output};
use crate::processor::Processor;
use crate::sequence_system::Key;

use super::{Document, Format};

#[derive(Serialize)]
struct InputRef<'a> {
    key: &'a Key,
    input: &'a Input,
}

#[derive(Serialize)]
struct ProcessorBodyRef<'a> {
    output: &'a Key,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<InputRef<'a>>,
    formula: &'a Output,
}

#[derive(Deserialize)]
struct InputBody {
    key: Key,
    input: Input,
}

#[derive(Deserialize)]
struct ProcessorBody {
    output: Key,
    #[serde(default)]
    inputs: Vec<InputBody>,
    formula: Output,
}

impl Serialize for Processor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: ProcessorBodyRef {
                output: self.output(),
                inputs: self
                    .declared_inputs()
                    .iter()
                    .map(|(key, input)| InputRef { key, input })
                    .collect(),
                formula: self.formula(),
            },
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

        let mut declared = BTreeSet::new();
        let mut processor = Self::new(body.output, body.formula);
        for InputBody { key, input } in body.inputs {
            if !declared.insert(key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the input `{key}` is declared twice in the processor"
                )));
            }
            processor = processor.with_input(key, input);
        }
        processor.check().map_err(de::Error::custom)?;

        Ok(processor)
    }
}
