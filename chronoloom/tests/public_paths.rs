//! Every public path the crate exposes still resolves.
//!
//! The modules were split into directories, which moves items between files.
//! This fails to compile if any of those moves changed a path a dependent could
//! be importing — including the deep `module::item` paths, not only the
//! re-exports at the crate root.
#![allow(unused_imports)]

use chronoloom::primitives::interval::{IntervalError, TimeIntervalEvent};
use chronoloom::primitives::time_point::TimePointEvent;
use chronoloom::primitives::Timestamp;
use chronoloom::processing::{
    Condition, Edge, Events, Formula, OpenEnd, Pattern, ProcessError, Recipe,
};
use chronoloom::sequence_system::{
    AnyPointSequence, Key, PointValue, RecordError, Sequence, SequenceSystem,
};
use chronoloom::sequences::attribute::{Attribute, SetOperation};
use chronoloom::sequences::interval::TimeIntervalSequence;
use chronoloom::sequences::time_point::TimePointSequence;
use chronoloom::stream_system::{
    Batch, Chunk, Live, LiveReport, Mode, Report, Update, UpdateError,
};
use chronoloom::{
    AnyPointSequence as RootAnyPointSequence, Attribute as RootAttribute,
    IntervalError as RootError, Key as RootKey, PointValue as RootPointValue,
    RecordError as RootRecordError, Sequence as RootSequence, SequenceSystem as RootSequenceSystem,
    SetOperation as RootSetOperation, TimeIntervalEvent as RootIntervalEvent,
    TimeIntervalSequence as RootIntervalSequence, TimePointEvent as RootPointEvent,
    TimePointSequence as RootPointSequence, Timestamp as RootTimestamp,
};
use chronoloom::{
    Batch as RootBatch, Chunk as RootChunk, Condition as RootCondition, Edge as RootEdge,
    Events as RootEvents, Formula as RootFormula, Live as RootLive, LiveReport as RootLiveReport,
    Mode as RootMode, OpenEnd as RootOpenEnd, Pattern as RootPattern,
    ProcessError as RootProcessError, Processor as RootProcessor, Recipe as RootRecipe,
    Report as RootReport, StreamSystem as RootStreamSystem, Update as RootUpdate,
    UpdateError as RootUpdateError,
};

/// Compiling the imports above is the real assertion; this only gives the
/// harness something to run.
#[test]
fn every_public_path_resolves() {
    let _: Timestamp = 0;
}
