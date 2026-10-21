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
use chronoloom::sequences::attribute::{Attribute, SetOperation};
use chronoloom::sequences::interval::TimeIntervalSequence;
use chronoloom::sequences::time_point::TimePointSequence;
use chronoloom::{
    Attribute as RootAttribute, IntervalError as RootError, SetOperation as RootSetOperation,
    TimeIntervalEvent as RootIntervalEvent, TimeIntervalSequence as RootIntervalSequence,
    TimePointEvent as RootPointEvent, TimePointSequence as RootPointSequence,
    Timestamp as RootTimestamp,
};

/// Compiling the imports above is the real assertion; this only gives the
/// harness something to run.
#[test]
fn every_public_path_resolves() {
    let _: Timestamp = 0;
}
