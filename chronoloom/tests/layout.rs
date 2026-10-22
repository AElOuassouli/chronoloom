//! The `#[repr(C)]` layouts the event types promise, which code outside Rust
//! relies on to read a sequence's memory in place.

use core::mem::{align_of, size_of};

use chronoloom::primitives::{TimeIntervalEvent, TimePointEvent};
use chronoloom::sequences::{TimeIntervalSequence, TimePointSequence};

/// How far into `outer` the value `inner` points, in bytes.
fn offset_of_reference<O, I>(outer: &O, inner: &I) -> usize {
    inner as *const I as usize - outer as *const O as usize
}

#[test]
fn point_event_is_timestamp_then_value() {
    let event = TimePointEvent::new(7, 2.5_f64);

    assert_eq!(size_of::<TimePointEvent<f64>>(), 16);
    assert_eq!(align_of::<TimePointEvent<f64>>(), 8);
    assert_eq!(offset_of_reference(&event, event.value()), 8);

    // SAFETY: `repr(C)` puts the `i64` timestamp at offset 0.
    let timestamp = unsafe { *(&event as *const TimePointEvent<f64>).cast::<i64>() };
    assert_eq!(timestamp, 7);
}

#[test]
fn point_event_pads_a_narrow_value_to_the_timestamp_alignment() {
    let event = TimePointEvent::new(7, true);

    assert_eq!(size_of::<TimePointEvent<bool>>(), 16);
    assert_eq!(size_of::<TimePointEvent<i64>>(), 16);
    assert_eq!(offset_of_reference(&event, event.value()), 8);
}

#[test]
fn valueless_span_is_exactly_start_then_end() {
    let span = TimeIntervalEvent::span(3, 9).unwrap();

    assert_eq!(size_of::<TimeIntervalEvent<()>>(), 16);

    // SAFETY: `repr(C)` with a zero-sized value is exactly two `i64`s.
    let bounds = unsafe { *(&span as *const TimeIntervalEvent<()>).cast::<[i64; 2]>() };
    assert_eq!(bounds, [3, 9]);
}

#[test]
fn sequences_are_contiguous_arrays_of_events() {
    let points = TimePointSequence::from_events(
        "readings",
        vec![
            TimePointEvent::new(20, 2.0_f64),
            TimePointEvent::new(10, 1.0),
        ],
    );
    let spans = TimeIntervalSequence::from_spans(
        "up",
        vec![
            TimeIntervalEvent::span(20, 30).unwrap(),
            TimeIntervalEvent::span(0, 10).unwrap(),
        ],
    );

    // SAFETY: the slice holds two `repr(C)` events of two eight-byte fields each.
    let raw_points = unsafe { *points.as_slice().as_ptr().cast::<[[u64; 2]; 2]>() };
    let raw_spans = unsafe { *spans.as_slice().as_ptr().cast::<[[i64; 2]; 2]>() };

    assert_eq!(raw_points[0][0], 10);
    assert_eq!(f64::from_bits(raw_points[1][1]), 2.0);
    assert_eq!(raw_spans, [[0, 10], [20, 30]]);
}
