//! Processors run in batch, the way the tests before stream systems ran a set
//! of outputs: a stream system holding the members of a system as streams and
//! the processors, each added after the outputs it reads.

use chronoloom::stream_system::{StreamSystem, UpdateError};
use chronoloom::{ProcessError, Processor, SequenceSystem};

/// A batch stream system of `streams` and `processors`, or the error of the
/// first processor that cannot be added once every other that can has been.
pub fn run(
    streams: &SequenceSystem,
    processors: &[Processor],
) -> Result<StreamSystem, ProcessError> {
    let mut system = StreamSystem::new();
    for (key, sequence) in streams {
        system
            .insert_stream(key.clone(), sequence.clone())
            .expect("a system with no processor takes any stream");
    }

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
