//! A panic inside `update` undoes the update before it goes on.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::processor::Processor;
use crate::store::Sequence;
use crate::{Condition, Formula, Pattern, TimeIntervalSequence, TimePointEvent, TimePointSequence};

use super::{StreamSystem, Update};

/// A room whose `presence` output is held as a point sequence, though the
/// processor gives spans: the first chunk it appends cannot be joined to it,
/// and `append_derived` panics after the events were recorded, the frontier
/// moved and the processor stepped.
fn room_whose_output_cannot_take_a_chunk() -> StreamSystem<super::Live> {
    let mut room = StreamSystem::live(0);
    room.insert_stream("motion", TimePointSequence::<&str>::new("motion"))
        .unwrap();
    room.add_processor(Processor::new(
        "presence",
        Formula::between(
            Pattern::new("motion", Condition::Equals("start")),
            Pattern::new("motion", Condition::Equals("end")),
        ),
    ))
    .unwrap();
    room.members.insert_derived(
        "presence".into(),
        Sequence::from(TimePointSequence::<i64>::new("presence")),
        None,
    );
    room
}

fn first_update() -> Update {
    let mut update = Update::new();
    update
        .points(
            "motion",
            [
                TimePointEvent::new(10, "start"),
                TimePointEvent::new(25, "end"),
            ],
        )
        .advance("motion", 30);
    update
}

#[test]
fn a_panic_in_an_update_changes_nothing_and_the_system_stays_usable() {
    let mut room = room_whose_output_cannot_take_a_chunk();
    let before = room.clone();

    let outcome = catch_unwind(AssertUnwindSafe(|| room.update(first_update())));
    assert!(outcome.is_err(), "the panic goes on");
    assert_eq!(room, before);
    // The system comparison leaves out the running state of the processors.
    assert_eq!(room.mode.nodes[0].read, before.mode.nodes[0].read);
    assert_eq!(
        room.mode.nodes[0].recipe.frontier(),
        before.mode.nodes[0].recipe.frontier()
    );

    // With the output member right, the same update runs the processor again,
    // and gives what it gives on a system that never panicked.
    let mut never_panicked = before.clone();
    for system in [&mut room, &mut never_panicked] {
        system.members.insert_derived(
            "presence".into(),
            Sequence::from(TimeIntervalSequence::from_spans("presence", Vec::new())),
            None,
        );
    }
    let expected = never_panicked.update(first_update()).unwrap();
    assert_eq!(room.update(first_update()).unwrap(), expected);
    assert_eq!(room, never_panicked);
    assert_eq!(room.mode.nodes[0].read, never_panicked.mode.nodes[0].read);
    assert_eq!(
        room.mode.nodes[0].recipe.frontier(),
        never_panicked.mode.nodes[0].recipe.frontier()
    );
    assert!(!expected.chunks().is_empty());
}
