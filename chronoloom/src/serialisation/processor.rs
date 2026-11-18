//! A [`Processor`] as a document: its outputs in key order.

use std::collections::BTreeSet;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::processing::{Input, Output, Processor};
use crate::sequence_system::Key;

use super::{Document, Format};

#[derive(Serialize)]
struct OutputRef<'a> {
    key: &'a Key,
    output: &'a Output,
}

#[derive(Serialize)]
struct InputRef<'a> {
    key: &'a Key,
    input: &'a Input,
}

#[derive(Serialize)]
struct ProcessorBodyRef<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<InputRef<'a>>,
    outputs: Vec<OutputRef<'a>>,
}

#[derive(Deserialize)]
struct OutputBody {
    key: Key,
    output: Output,
}

#[derive(Deserialize)]
struct InputBody {
    key: Key,
    input: Input,
}

#[derive(Deserialize)]
struct ProcessorBody {
    #[serde(default)]
    inputs: Vec<InputBody>,
    outputs: Vec<OutputBody>,
}

impl Serialize for Processor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Document {
            format: Format,
            body: ProcessorBodyRef {
                inputs: self
                    .inputs()
                    .iter()
                    .map(|(key, input)| InputRef { key, input })
                    .collect(),
                outputs: self
                    .outputs()
                    .iter()
                    .map(|(key, output)| OutputRef { key, output })
                    .collect(),
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
    /// Refuses a key that appears twice among the outputs or the inputs, and a processor whose outputs read
    /// each other in a loop.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Document {
            format: Format,
            body,
        } = Document::<ProcessorBody>::deserialize(deserializer)?;

        let mut seen = BTreeSet::new();
        let mut processor = Self::new();
        for OutputBody { key, output } in body.outputs {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the output `{key}` appears twice in the processor"
                )));
            }
            processor.insert(key, output);
        }
        let mut declared = BTreeSet::new();
        for InputBody { key, input } in body.inputs {
            if !declared.insert(key.clone()) {
                return Err(de::Error::custom(format_args!(
                    "the input `{key}` is declared twice in the processor"
                )));
            }
            processor.insert_input(key, input);
        }
        processor.check().map_err(de::Error::custom)?;

        Ok(processor)
    }
}
